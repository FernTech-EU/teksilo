# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""A screen reader's own actions on a control that left the tree and came back.

A node that leaves the tree a reader sees and comes back reaches the AT-SPI
adapter under an id it has never had (`WidgetTree::deliver_accessibility`),
so the adapter names it by that id when a screen reader acts on it, and
`teksilo-app` takes the request back to the tree's own id
(`WidgetTree::resolve_adapter_action`) before it looks the widget up. These
acts reach the returned control through AT-SPI alone, grab_focus and click,
so they fail if the way back is missing: the request would name no widget, or
the wrong one.
"""

from __future__ import annotations

from reader_lib.checks import custom, focused, said
from reader_lib.scenario import Scenario
from scenarios.tabs import focus_target_was_defunct, nodes, setup, to_tab

TW = "tab-widget"


def vertical_strip():
    """The tab strip is vertical after the act: Toggle orientation ran."""
    def run(act):
        lists = [n for n in nodes(act.tree, role="page tab list")]
        states = [sorted(n.get("states", [])) for n in lists]
        return any("vertical" in s for s in states), [f"page tab lists: {states}"]
    return custom("the tab strip is vertical", run, needs_tree=True)


def at_action_revisit(run):
    """Leave Settings, come back, and act on its returned panel's button
    through AT-SPI only."""
    run.wait_for(role="page tab", name="Welcome")
    to_tab(run, 1)
    with setup(run, "Enter into the Settings panel (first visit)"):
        run.key("Return")
    with setup(run, "back to the Settings tab, Right to Doc 1, Left to Settings"):
        run.grab_focus(role="page tab", name="Settings")
        run.wait(0.5)
        run.key("Right")
        run.wait(0.5)
        run.key("Left")
    with run.act("AT-SPI grab_focus on the returned panel's button",
                 [focused(role="push button", name="Toggle orientation"),
                  said("Toggle orientation"), focus_target_was_defunct()],
                 should="focus lands on the button the reader asked for, and Orca says it"):
        run.grab_focus(role="push button", name="Toggle orientation")
    with run.act("AT-SPI click on the returned panel's button",
                 [vertical_strip()],
                 should="the button runs: the strip turns vertical", tree=True):
        run.action("click", role="push button", name="Toggle orientation")


SCENARIOS = [
    Scenario("fix-node-ids-at-action-revisit", TW, at_action_revisit,
             "grab_focus and click through AT-SPI on a panel's button after it came back"),
]
