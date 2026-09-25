# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Verifier's extra acts for multi-window and internationalization (the
sweep's own acts are in `windows_i18n.py`; these only add what its scenarios
did not do).

* `verify-winintl-mw-tabwalk`: Tab through the main window with the seat held
  steady (no window is opened, so the harness crash cannot occur): what each
  stop says, including the toolbar entry (`Toolbar tool bar`, sweep
  winintl-14), the dimming button that has no action, and the Open help
  button whose name promises F11.
* `verify-winintl-intl-switcher-escape`: the LanguageSwitcher list, opened
  with Alt+Down, arrowed Down (sweep winintl-04 says this already switches the
  app), then Escape: does the switch stand, i.e. can a reader who was only
  browsing take it back?
* `verify-winintl-intl-switcher-closed`: Down on the *closed* combo box.
* `verify-winintl-intl-heading-defunct`: the default-size window; Tab down to
  + 1 (the heading, greeting and intro scroll out and are announced defunct),
  Shift+Tab back to English; then switch to French and see whether Orca
  handles the heading's rename (sweep winintl-01 / winintl-15, on labels,
  at the example's own window size).
"""

from __future__ import annotations

from reader_lib.checks import _event_line, _is_focus, custom, event, focused, said
from reader_lib.orca import utterances
from reader_lib.scenario import Scenario, tab_walk
from scenarios.windows_i18n import SteadySeat, names_in_tree

MW = "multi-window"
INTL = "internationalization"


def speech():
    def run(act):
        heard = utterances(act.orca)
        return True, [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r} at {u.stamp}"
                      for u in heard] or ["Orca said nothing in this act"]
    return custom("Orca's speech in this act (listed)", run, needs_orca=True)


def focus_record():
    def run(act):
        moves = [_event_line(act, e) for e in act.events if _is_focus(e)]
        return True, moves or ["no focus change on the bus in this act"]
    return custom("focus changes on the bus (listed)", run)


def orca_defunct_drops():
    def run(act):
        drops = [f"{line.stamp} {line.text}" for line in act.orca if line.is_defunct_drop]
        return True, drops or ["Orca dropped nothing as defunct"]
    return custom("Orca's defunct drops in this act (listed)", run, needs_orca=True)


def label_names(box: dict, key: str):
    def run(act):
        stack = [act.tree] if act.tree else []
        names = []
        while stack:
            node = stack.pop()
            if node.get("role") in ("label", "push button", "combo box"):
                names.append(f"[{node.get('role')}] {node.get('name')!r}")
            stack.extend(reversed(node.get("children", [])))
        box[key] = names
        return True, names
    return custom("labels/buttons in the tree after the act (listed)", run, needs_tree=True)


# ---------------------------------------------------------------------------


def mw_tabwalk(run):
    seat = SteadySeat(run)
    try:
        run.wait_for(role="push button", name="Open help (F1) / Toggle fullscreen (F11)")
        tab_walk(run, stops=6)
        run.grab_focus(role="push button", name="This panel dims when the window is inactive")
        with run.act("Space on the dimming button",
                     [speech(), focus_record()],
                     should="(record) whether activating it does anything a reader hears"):
            run.key("space")
        run.grab_focus(role="push button", name="Open help (F1) / Toggle fullscreen (F11)")
        with run.act("the Open help button's attributes",
                     [custom("record the button's attributes", lambda act: (True, [
                         repr(run.find(role="push button",
                                       name="Open help (F1) / Toggle fullscreen (F11)"))]))],
                     should="(record) is its F1 shortcut exposed", tree=True):
            run.wait(0.1)
    finally:
        seat.close()


def intl_switcher_escape(run):
    combo = {"role": "combo box", "name": "Language"}
    run.wait_for(**combo)
    run.grab_focus(**combo)
    with run.act("Alt+Down opens the list", [speech()], should="the list opens"):
        run.key("Alt+Down")
    with run.act("Down to français", [speech(),
                                     event("object:property-change:accessible-name",
                                           role="label", name_contains="Bonjour")],
                 should="(sweep winintl-04) moving the highlight already switches the app"):
        run.key("Down")
    box: dict = {}
    with run.act("Escape closes the list",
                 [speech(), focus_record(), label_names(box, "after_escape"),
                  names_in_tree("Hello, Alice!")],
                 should="Escape cancels a browse: the app should be back in English",
                 tree=True):
        run.key("Escape")


def intl_switcher_closed(run):
    combo = {"role": "combo box", "name": "Language"}
    run.wait_for(**combo)
    run.grab_focus(**combo)
    with run.act("Down on the closed combo box",
                 [speech(), focus_record(),
                  event("object:property-change:accessible-name", role="label",
                        name_contains="Bonjour")],
                 should="(record) does Down on the closed box switch the language", tree=True):
        run.key("Down")
    with run.act("Escape", [speech(), names_in_tree("Bonjour, Alice !")],
                 should="(record) whether the switch stands", tree=True):
        run.key("Escape")


def intl_heading_defunct(run):
    run.wait_for(role="push button", name="English")
    run.grab_focus(role="push button", name="Trailing")
    with run.act("Tab x4 to + 1 (the intro scrolls out)",
                 [event("object:state-changed:defunct", role="label",
                        name_contains="Teksilo i18n Showcase"), speech()],
                 should="(record) the heading leaves the tree and is announced defunct"):
        run.key("Tab", "Tab", "Tab", "Tab", gap=0.6)
    with run.act("Shift+Tab x8 back to English",
                 [focused(role="push button", name="English"), speech(), orca_defunct_drops()],
                 should="focus goes back up; the heading scrolls back in", tree=True):
        run.key(*(["Shift+Tab"] * 8), gap=0.6)
    with run.act("AT-SPI click on Français",
                 [event("object:property-change:accessible-name", role="label",
                        name_contains="Vitrine"), orca_defunct_drops(), speech()],
                 should="(record) Orca drops the renamed heading's events as defunct"):
        run.action("click", role="push button", name="Français")


def intl_combo_reopen(run):
    """The Theme combo box's list opened, closed with Escape and opened again:
    its rows leave the tree (announced defunct) when it closes, and come back
    when it reopens. Theme rather than Language, so an arrow press does not
    rename the whole page."""
    combo = {"role": "combo box", "name": "Theme"}
    run.wait_for(**combo)
    run.grab_focus(**combo)
    for n in (1, 2, 3):
        with run.act(f"Alt+Down opens the list ({n})", [speech(), orca_defunct_drops()],
                     should="the list opens and the reader hears it and its current row",
                     tree=(n == 2)):
            run.key("Alt+Down")
        with run.act(f"Down to the next row ({n})", [speech(), orca_defunct_drops()],
                     should="the reader hears the next row"):
            run.key("Down")
        with run.act(f"Escape closes the list ({n})", [speech(), orca_defunct_drops()],
                     should="the list closes"):
            run.key("Escape")


SCENARIOS = [
    Scenario("verify-winintl-intl-combo-reopen", INTL, intl_combo_reopen,
             "Theme combo: open, arrow, close, three times", lang="en_US.UTF-8"),
    Scenario("verify-winintl-mw-tabwalk", MW, mw_tabwalk,
             "Tab through the main window, seat steady; the dimming and Open help buttons"),
    Scenario("verify-winintl-intl-switcher-escape", INTL, intl_switcher_escape,
             "LanguageSwitcher: Alt+Down, Down, Escape: does the switch stand",
             lang="en_US.UTF-8"),
    Scenario("verify-winintl-intl-switcher-closed", INTL, intl_switcher_closed,
             "LanguageSwitcher: Down on the closed box, then Escape", lang="en_US.UTF-8"),
    Scenario("verify-winintl-intl-heading-defunct", INTL, intl_heading_defunct,
             "default window: the heading scrolls out and back, then is renamed",
             lang="en_US.UTF-8"),
]
