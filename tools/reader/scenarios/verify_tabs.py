# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Verifier's extra acts for tab-widget and tab-migration (the sweep's own acts
are in `tabs.py`; these only add what its scenarios did not do).

* `verify-tabs-revisit-tab`: the returned-panel silence (sweep tabs-01) reached
  by the ordinary route, Tab from the tab into the panel, rather than Enter;
  then AT-SPI grab_focus on the returned button, a screen reader's own focus.
* `verify-tabs-activate-other`: the tab-bar rebuild with focus elsewhere
  (sweep tabs-06) triggered by Enter and by an AT-SPI click instead of Space.
  Orca returns early from `onSelectionChanged` when the last key it saw was
  Space (`default.py`, `if keyString == "space": return`), and the harness's
  keys never reach Orca, so only a non-Space activation says what a real
  reader gets.
* `verify-tabs-vertical`: the vertical strip (the sweep did not press
  Up / Down): arrows, End, Alt+Up, and the move labels in the context menu.
* `verify-tabs-enter-empty-panel`: Enter on a tab whose panel has no
  focusable control (Welcome).
* `verify-tabs-close-middle`: close Doc 2 (the middle document) and Doc 3
  (the last tab): which tab is selected afterwards.
* `verify-tabs-harness-cursor`: a key pressed outside any act, then an act;
  is the first key's speech credited to the act (the sweep's harness issue)?
"""

from __future__ import annotations

from reader_lib.checks import _event_line, custom, focused, said
from reader_lib.orca import utterances
from scenarios.tabs import (TW, _tablist, announcements, dump, focus_target_was_defunct,
                            nodes, orca_log, selected_tab, setup, spoke, summary, tab_order,
                            to_tab)
from reader_lib.scenario import Scenario


def speech() -> object:
    """Every utterance of the act (always passes: a record)."""
    def run(act):
        heard = utterances(act.orca)
        return True, [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r} at {u.stamp}"
                      for u in heard] or ["Orca said nothing in this act"]
    return custom("Orca's speech in this act (listed)", run, needs_orca=True)


def orca_lag() -> object:
    def run(act):
        return True, [f"orca window {act.orca_start} .. {act.orca_end}, act start "
                      f"{act.start_wall}, lag {act.orca_lag_ms} ms"]
    return custom("where Orca's window for this act starts (a record)", run)


def focus_record() -> object:
    def run(act):
        from reader_lib.checks import _is_focus
        moves = [_event_line(act, e) for e in act.events if _is_focus(e)]
        return True, moves or ["no focus change on the bus in this act"]
    return custom("focus changes on the bus (listed)", run)


# ---------------------------------------------------------------------------


def revisit_tab(run):
    run.wait_for(role="page tab", name="Welcome")
    to_tab(run, 1)
    with run.act("Tab x5 from Settings into its panel (first visit)",
                 [focused(role="push button", name="Toggle orientation"),
                  said("Toggle orientation"), speech()],
                 should="the Tab route reaches the panel's button, spoken"):
        run.key("Tab", "Tab", "Tab", "Tab", "Tab", gap=0.35)
    with setup(run, "AT focus back on the Settings tab"):
        run.grab_focus(role="page tab", name="Settings")
    with run.act("Right to Doc 1", [focused(role="page tab", name="Doc 1"), speech()],
                 should="Doc 1 selected; Settings' panel leaves the tree"):
        run.key("Right")
    with run.act("Left back to Settings", [focused(role="page tab", name="Settings"),
                                           speech()],
                 should="Settings selected; its panel returns"):
        run.key("Left")
    with run.act("Tab x5 into the returned panel",
                 [focused(role="push button", name="Toggle orientation"),
                  said("Toggle orientation"), focus_target_was_defunct(),
                  orca_log("Ignoring defunct"), speech()],
                 should="the reader hears the button as on the first visit"):
        run.key("Tab", "Tab", "Tab", "Tab", "Tab", gap=0.35)
    with setup(run, "AT focus back on the Settings tab"):
        run.grab_focus(role="page tab", name="Settings")
    with run.act("AT-SPI grab_focus on the returned Toggle orientation",
                 [focused(role="push button", name="Toggle orientation"),
                  said("Toggle orientation"), orca_log("Ignoring defunct"), speech()],
                 should="a screen reader's own focus request lands and is spoken"):
        run.grab_focus(role="push button", name="Toggle orientation")


def activate_other(run):
    run.wait_for(role="push button", name="+ New tab")
    with setup(run, "focus '+ New tab'"):
        run.grab_focus(role="push button", name="+ New tab")
    with run.act("Enter on '+ New tab'",
                 [speech(), focus_record(), orca_log("locus of focus"),
                  custom("the new tab Doc 4 is in the tree",
                         lambda act: (any(True for _ in nodes(act.tree, role="page tab",
                                                              name="Doc 4")),
                                      [f"page tabs: {[n.get('name') for n in nodes(act.tree, role='page tab')]}"]),
                         needs_tree=True)],
                 should="a new tab opens; focus stays on the button", tree=True):
        run.key("Return")
    with run.act("AT-SPI click on '+ New tab' (Orca's locus left where the last act put it)",
                 [speech(), focus_record(), orca_log("locus of focus", "redundant")],
                 should="a new tab opens; focus stays on the button"):
        run.action("click", role="push button", name="+ New tab")
    with setup(run, "Tab away and back, so Orca's locus is on '+ New tab' again"):
        run.key("Shift+Tab")
        run.wait(0.4)
        run.key("Tab")
    with run.act("AT-SPI click on '+ New tab' (Orca's locus on the button)",
                 [speech(), focus_record(), orca_log("locus of focus", "redundant")],
                 should="a new tab opens; focus stays on the button"):
        run.action("click", role="push button", name="+ New tab")
    with setup(run, "focus Orient"):
        run.grab_focus(role="push button", name="Orient")
    with run.act("Enter on Orient", [speech(), focus_record(), orca_log("locus of focus")],
                 should="the strip turns vertical; focus stays on Orient"):
        run.key("Return")
    with setup(run, "focus Sizing"):
        run.grab_focus(role="push button", name="Sizing")
    with run.act("AT-SPI click on Sizing", [speech(), focus_record(),
                                            orca_log("locus of focus")],
                 should="the tab sizing changes; focus stays where it was"):
        run.action("click", role="push button", name="Sizing")


def vertical(run):
    run.wait_for(role="push button", name="Orient")
    with setup(run, "Orient (vertical), then AT focus on Welcome"):
        run.action("click", role="push button", name="Orient")
        run.wait(1.0)
        run.grab_focus(role="page tab", name="Welcome")
    with run.act("Down: to Settings",
                 [focused(role="page tab", name="Settings"), said("Settings"),
                  selected_tab("Settings"),
                  custom("the tab list reads vertical",
                         lambda act: (_tablist(act) is not None
                                      and "vertical" in _tablist(act).get("states", []),
                                      [summary(_tablist(act))] if _tablist(act) else []),
                         needs_tree=True)],
                 should="Down moves focus and selection to the next tab", tree=True):
        run.key("Down")
    with run.act("Down: past Locked to Doc 1",
                 [focused(role="page tab", name="Doc 1"), said("Doc 1")],
                 should="the disabled tab is skipped"):
        run.key("Down")
    with run.act("Right in a vertical strip", [focus_record(), speech()],
                 should="(what Right does in a vertical strip)", tree=True):
        run.key("Right")
    with run.act("Up: back to Settings",
                 [focused(role="page tab", name="Settings"), said("Settings")],
                 should="Up moves back"):
        run.key("Up")
    with run.act("End: to Doc 3", [focused(role="page tab", name="Doc 3"), said("Doc 3")],
                 should="End reaches the last tab"):
        run.key("End")
    with run.act("Alt+Up on Doc 3",
                 [announcements(), speech(),
                  tab_order("Welcome", "Settings", "Locked", "Doc 1", "Doc 3", "Doc 2")],
                 should="Doc 3 moves up one place and the reader hears where it went",
                 tree=True):
        run.key("Alt+Up")
    with run.act("the context-menu key on Doc 3",
                 [dump("menu item", "the menu's items (vertical labels expected)"), speech()],
                 should="the moves are labelled Up/Down, not Left/Right", tree=True):
        run.key("Menu")
    with setup(run, "Escape"):
        run.key("Escape")


def enter_empty_panel(run):
    run.wait_for(role="page tab", name="Welcome")
    with run.act("Tab to Welcome", [focused(role="page tab", name="Welcome"), speech()],
                 should="focus on Welcome"):
        run.key("Tab")
    with run.act("Enter on Welcome (its panel has no focusable control)",
                 [focus_record(), speech()],
                 should="(where Enter takes focus when the panel has nothing focusable)",
                 tree=True):
        run.key("Return")
    with run.act("Tab from there", [focus_record(), speech()],
                 should="(where Tab goes next)"):
        run.key("Tab")


def close_middle(run):
    run.wait_for(role="page tab", name="Doc 2")
    to_tab(run, 3)
    with setup(run, "Delete on Doc 2"):
        run.key("Delete")
    run.wait_for(role="push button", name="Yes")
    with run.act("answer Yes: Doc 2 closes",
                 [focus_record(), speech(), selected_tab("Doc 3"),
                  dump("page tab", "page tabs after the close")],
                 should="the documented positional fallback selects the closed tab's "
                 "next neighbour, Doc 3", tree=True):
        run.action("click", role="push button", name="Yes")
    with setup(run, "AT focus on Doc 3, Delete"):
        run.grab_focus(role="page tab", name="Doc 3")
        run.wait(0.5)
        run.key("Delete")
    run.wait_for(role="push button", name="Yes")
    with run.act("answer Yes: Doc 3 (the last tab) closes",
                 [focus_record(), speech(), selected_tab("Doc 1"),
                  dump("page tab", "page tabs after the close")],
                 should="the last tab closed: its previous neighbour Doc 1 is selected",
                 tree=True):
        run.action("click", role="push button", name="Yes")


def harness_cursor(run):
    run.wait_for(role="page tab", name="Welcome")
    with run.act("Tab to Welcome", [speech(), orca_lag()], should="focus on Welcome"):
        run.key("Tab")
    # Outside any act, on purpose: the step the sweep's harness issue is about.
    run.key("Right")
    run.wait(0.3)
    with run.act("Right again (after a Right pressed outside any act)",
                 [speech(), orca_lag(), focused(role="page tab", name="Doc 1"),
                  custom("Orca's speech in this act is only about this act's Right "
                         "(Doc 1), not the out-of-act Right (Settings)",
                         lambda act: (not any("Settings" in u.text
                                              for u in utterances(act.orca)),
                                      [u.text for u in utterances(act.orca)]),
                         needs_orca=True)],
                 should="Orca's window starts at this act's own events"):
        run.key("Right")


def _four_new_tabs(run):
    run.wait_for(role="push button", name="+ New tab")
    with setup(run, "focus '+ New tab', four new tabs"):
        run.grab_focus(role="push button", name="+ New tab")
        run.wait(0.5)
        run.key("space", "space", "space", "space", gap=0.5)


def scroll_arrow_hide(run):
    """Press the focused 'Scroll tabs right' until the strip reaches its end:
    the arrow hides at the end, and it holds focus."""
    _four_new_tabs(run)
    with setup(run, "AT focus on Welcome, Tab to 'Scroll tabs right'"):
        run.grab_focus(role="page tab", name="Welcome")
        run.wait(0.5)
        run.key("Tab")
    run.wait_for(role="push button", name="Scroll tabs right")
    for i in range(6):
        with run.act(f"Space on the focused 'Scroll tabs right', press {i + 1}",
                     [focus_record(), speech(),
                      custom("'Scroll tabs right' is still in the tree",
                             lambda act: (any(True for _ in nodes(act.tree, role="push button",
                                                                  name="Scroll tabs right")),
                                          [f"buttons: {[n.get('name') for n in nodes(act.tree, role='push button')]}"]),
                             needs_tree=True)],
                     should="the strip scrolls; when the arrow hides at the end, focus moves "
                     "somewhere sensible and the reader is told", tree=True, record=1.8):
            run.key("space")
    with run.act("Tab from where focus is now", [focus_record(), speech()],
                 should="(where the reader is after the arrow hid)"):
        run.key("Tab")


def dropdown_realistic(run):
    """The dropdown as a reader meets it: focus on 'Show all tabs', Space opens
    it (focus goes into the list), then the screen reader's own click on an
    entry (the only route that works)."""
    _four_new_tabs(run)
    run.wait_for(role="push button", name="Show all tabs", timeout=5)
    with setup(run, "AT focus on 'Show all tabs', Space"):
        run.grab_focus(role="push button", name="Show all tabs")
        run.wait(0.6)
        run.key("space")
    with run.act("AT-SPI click on the 'Doc 6' entry",
                 [focus_record(), speech(), selected_tab("Doc 6"),
                  focused(role="page tab", name="Doc 6")],
                 should="Doc 6 is selected, the dropdown closes, and keyboard focus lands "
                 "on Doc 6 (or at least where Orca says it is)", tree=True):
        run.action("click", role="push button", name="Doc 6")
    with run.act("Tab from there", [focus_record(), speech()],
                 should="(where keyboard focus really is)"):
        run.key("Tab")


SCENARIOS = [
    Scenario("verify-tabs-scroll-arrow-hide", TW, scroll_arrow_hide,
             "press the focused scroll arrow until it hides"),
    Scenario("verify-tabs-dropdown-realistic", TW, dropdown_realistic,
             "open 'Show all tabs' from focus, pick an entry by AT click"),
    Scenario("verify-tabs-revisit-tab", TW, revisit_tab,
             "the returned-panel silence reached with Tab and with AT grab_focus"),
    Scenario("verify-tabs-activate-other", TW, activate_other,
             "bar rebuilds under Enter / AT click (not Space)"),
    Scenario("verify-tabs-vertical", TW, vertical,
             "the vertical strip: Up / Down / End / Alt+Up / menu labels"),
    Scenario("verify-tabs-enter-empty-panel", TW, enter_empty_panel,
             "Enter on a tab whose panel has nothing focusable"),
    Scenario("verify-tabs-close-middle", TW, close_middle,
             "close a middle and a last document: which tab is selected"),
    Scenario("verify-tabs-harness-cursor", TW, harness_cursor,
             "a key outside any act, then an act: where Orca's window starts"),
]
