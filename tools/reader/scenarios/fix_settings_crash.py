# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""widget-catalog's Settings tab with telemetry configured: the consent
switches `PrivacySettings` builds, as a screen reader meets them.

Every debug build used to exit with status 101 as this tab's tree was built:
the consent rows name their switch through `labelled_by`, and `Toggle`'s own
"missing an accessible label" assertion could not see it. The sweep's
`catalog-c-settings-launch` only asks that the tab comes up; this one also
asks that each switch is reached, named by its row, and spoken with its state.

The switches sit below the fold, and `accesskit_consumer`'s `common_filter`
leaves out a clipped child wholly outside its parent's box unless a sibling
next to it is inside (`filters.rs:65-88`), so the launch tree cannot show
them. A reader reaches them as the scenario does: by Tab, from the last
control above them, which scrolls the first one into view.
"""

from __future__ import annotations

from reader_lib.checks import focused, in_tree, said
from reader_lib.scenario import Scenario
from scenarios.catalog_c import alive, setup

PKG = "widget-catalog"

SWITCHES = ("Anonymous usage metrics", "Crash reports", "Feature flags")


def consent(run):
    # The frame can reach the bus before its content does.
    run.wait_for(role="spin button", name="Text size", timeout=30.0)
    with run.act("after launch on the Settings tab",
                 [alive(), in_tree(role="spin button", name_contains="Text size")],
                 should="the tab stays up and is read",
                 tree=True, record=0.5):
        pass
    # The last Tab stop above the consent rows. A row's Reset is not a Tab
    # stop while the row holds its default, so it cannot be the start.
    rebinds = run.find_all(role="push button", name="Rebind 2nd")
    setup(run, "focus the last shortcut row's Rebind 2nd button",
          lambda: run.grab_focus(role="push button", name="Rebind 2nd", nth=len(rebinds) - 1))
    with run.act("Tab to the first consent switch",
                 [focused(role="toggle button", name=SWITCHES[0]),
                  said(SWITCHES[0]), said("not pressed"), alive()]
                 + [in_tree(role="toggle button", name=name) for name in SWITCHES],
                 should="the switch takes focus and the reader says its name and that it "
                        "is off; every consent switch is named by its row",
                 tree=True, record=2.0):
        run.key("Tab")


SCENARIOS = [
    Scenario("fix-settings-crash-consent", PKG, consent,
             "settings tab: the telemetry consent switches, named and spoken",
             args=["--tab", "settings"]),
]
