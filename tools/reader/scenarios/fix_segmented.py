# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""The SegmentedControl's chosen segment, as a reader hears it.

A segment is a `Role::RadioButton`, and every adapter reads a radio button's
state from AccessKit's `toggled`: AT-SPI sets CHECKED from it
(`accesskit_atspi_common` `node.rs:366-372`), UIA takes `IsSelected` from it
(`accesskit_windows` `node.rs:648-675`), macOS its value
(`accesskit_macos` `node.rs:344-352`). Orca 46.1's radio phrase is
"selected radio button" when CHECKED is set and "not selected radio button"
when it is not (`orca/generator.py:661-676`).

A segment used to carry `selected` and no `toggled`. So the chosen segment was
read "not selected radio button" (catalog-a-10, charts-08, sceneetc-12), and
the adapter raised `selection-changed` on the group whenever a control entered
the tree or changed its choice (`adapter.rs:79-81`, `318-320`), which Orca
answers by moving its locus of focus to the chosen segment and speaking it
(`orca/scripts/default.py:1540-1602`): five cut "not selected radio button"
at chart-demo's launch, and "Overview" said while the reader Tabbed onto a
slider in widget-catalog.

These scenarios state what a reader should get instead.
"""

from reader_lib.checks import custom, focused, no_event, not_said, said
from reader_lib.orca import normalized, utterances
from reader_lib.scenario import Scenario


def says_checked():
    """Orca speaks the radio state of a checked radio button, and nothing cut
    it. `said("selected radio button")` would not do: "not selected radio
    button" contains it."""
    def run(act):
        spoken = utterances(act.orca)
        ok = any(normalized(u.text) == normalized("selected radio button") and not u.cut
                 for u in spoken)
        return ok, [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                    for u in spoken] or ["Orca said nothing"]
    return custom("Orca says 'selected radio button', uncut", run, needs_orca=True)


def _walk(tree):
    stack = [tree] if tree else []
    while stack:
        node = stack.pop()
        yield node
        stack.extend(node.get("children", []))


def checked(name: str, want: bool = True):
    """The radio button `name` is, or is not, CHECKED in the tree a reader
    walks after the act."""
    def run(act):
        found = [n for n in _walk(act.tree)
                 if n.get("role") == "radio button" and n.get("name") == name]
        if not found:
            return False, [f"no [radio button] {name!r} in the tree"]
        states = found[0].get("states", [])
        return ("checked" in states) == want, [f"[radio button] {name!r} states={states}"]
    return custom(f"[radio button] {name!r} is {'' if want else 'not '}checked", run,
                  needs_tree=True)


def quiet_launch(run):
    """What the launch act should have given: no selection-changed from the
    SegmentedControls, and no radio state spoken for a control nobody is on.
    The launch act is the harness's own, so its checks are added here."""
    run.acts[0].expect += [no_event("object:selection-changed"), not_said("not selected")]


def catalog(run):
    """widget-catalog's Inputs page: the "Example choice" control, then the
    overflow control below the fold."""
    run.wait_for(role="slider", name="Vertical slider")
    with run.act("(scene) focus the vertical slider through AT-SPI",
                 should="scene setting, not judged", settle=0.3, record=1.0):
        run.grab_focus(role="slider", name="Vertical slider")
    with run.act("Tab into the segmented control",
                 [focused(role="radio button", name="First"), said("First"), says_checked(),
                  not_said("not selected"), checked("First"), checked("Second", False)],
                 should="'First, selected radio button'", tree=True):
        run.key("Tab")
    with run.act("Right: select the second segment",
                 [focused(role="radio button", name="Second"), said("Second"),
                  says_checked(), not_said("not selected"),
                  no_event("object:selection-changed"),
                  checked("Second"), checked("First", False)],
                 should="'Second, selected radio button', and First no longer checked",
                 tree=True):
        run.key("Right")
    with run.act("Tab to the width slider below it (the page scrolls)",
                 [focused(role="slider", name="Segmented control width"),
                  said("Segmented control width"), not_said("Overview"),
                  no_event("object:selection-changed")],
                 should="the reader hears the slider it is on, and not the segment of the "
                        "control that scrolled into view"):
        run.key("Tab")
    with run.act("Tab to the overflow segmented control",
                 [focused(role="radio button", name="Overview"), said("Overview"),
                  says_checked(), not_said("not selected"), checked("Overview")],
                 should="'Overview, selected radio button'", tree=True):
        run.key("Tab")


def charts(run):
    """chart-demo: the chart kind and the ChartStyle toggle, from launch."""
    quiet_launch(run)
    with run.act("(scene) Tab to the Theme combo box", should="scene setting, not judged",
                 settle=0.3, record=1.0):
        run.key("Tab")
    with run.act("Tab to the chart kind",
                 [focused(role="radio button", name="Bars"), said("Bars"), says_checked(),
                  not_said("not selected"), checked("Bars"), checked("Default")],
                 should="'Bars, selected radio button'", tree=True):
        run.key("Tab")
    with run.act("Right: Lines",
                 [focused(role="radio button", name="Lines"), said("Lines"), says_checked(),
                  not_said("not selected"), no_event("object:selection-changed"),
                  checked("Lines"), checked("Bars", False)],
                 should="'Lines, selected radio button', and Bars no longer checked",
                 tree=True):
        run.key("Right")
    with run.act("Tab to the chart style toggle",
                 [focused(role="radio button", name="Default"), said("Default"),
                  says_checked(), not_said("not selected")],
                 should="'Default, selected radio button'"):
        run.key("Tab")
    with run.act("Right: Gradient theme",
                 [focused(role="radio button", name="Gradient theme"), said("Gradient theme"),
                  says_checked(), not_said("not selected"),
                  no_event("object:selection-changed"),
                  checked("Gradient theme"), checked("Default", False)],
                 should="'Gradient theme, selected radio button'", tree=True):
        run.key("Right")


SCENARIOS = [
    Scenario("fix-segmented-catalog", "widget-catalog", catalog,
             "the Inputs page's segmented controls: the chosen segment is checked",
             args=["--tab", "inputs"]),
    Scenario("fix-segmented-charts", "chart-demo", charts,
             "chart-demo's chart kind and style toggles: a quiet launch, the chosen "
             "segment checked"),
]
