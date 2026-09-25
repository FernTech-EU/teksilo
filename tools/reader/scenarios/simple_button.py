# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""simple-button: the smallest thing a reader can meet, and the harness's own
check that it can hear anything at all."""

from reader_lib.checks import focused, said
from reader_lib.scenario import Scenario


def body(run):
    with run.act("Tab to the button", [focused(role="push button", name="Click Me"),
                                       said("Click Me")],
                 should="focus moves to the button and the reader says its name and role"):
        run.key("Tab")
    with run.act("activate it with Space", should="the button's action runs"):
        run.key("space")
    with run.act("activate it through AT-SPI",
                 should="a screen reader's own activation reaches the handler"):
        run.action("click", role="push button", name="Click Me")


SCENARIOS = [Scenario("simple-button", "simple-button", body, "one button")]
