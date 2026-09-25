# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""chart-demo: a grouped BarChart, a LineChart and a donut PieChart behind a
"Bars / Lines / Donut" SegmentedControl, a "Default / Gradient theme"
ChartStyle toggle, an interactive legend, and two live LineCharts fed every
600 ms (`examples/chart_demo/src/main.rs`).

What a reader should get (`crates/teksilo-charts/src/{bar,line,pie}_chart.rs`,
`hit.rs`, `legend.rs`):

* each chart is one Tab stop, `Role::GraphicsDocument` (AT-SPI "document
  frame") named "Bar chart: 3 series, 4 categories" and the like;
* every visible datum is a `Role::GraphicsObject` child (AT-SPI "panel")
  named "<series>, <category>: <value>" (`hit::mark_description`), with a
  Click action;
* arrows / Home / End move a virtual focus across the marks
  (`hit::drive_readout_keys`): the chart publishes the reached mark as its
  `active_descendant` AND says it through `ctx.announce` (the framework
  announcer: K2 on main);
* Enter / Space select the reached mark into the chart's `ChartSelection`;
* an interactive legend is a list of `Role::CheckBox` rows, one per series,
  toggled with Space (KeyDown arms, KeyUp fires) or a tap;
* the hover tooltip's keyboard equivalent is the same readout card, moved by
  the arrows.

Tab order at launch: Theme combo box, Bars (chart kind), Default (chart
style), the bar chart, the three legend rows, Clear selection, Add series,
Remove series, Refresh data, Running (feed), x4 (bucket), Mean (fn), the raw
live chart, the rollup live chart.
"""

from reader_lib.checks import (custom, event, focused, in_tree, no_event, not_in_tree,
                               not_said, said, said_once)
from reader_lib.orca import utterances
from reader_lib.run import RunError
from reader_lib.scenario import Scenario

SERIES = ["Revenue", "Cost", "Profit"]
QUARTERS = ["Q1", "Q2", "Q3", "Q4"]
PIE_LABELS = ["Storage", "Apps", "System", "Cache", "Free"]


def quarter_value(seed: int, si: int, i: int) -> int:
    """`quarter_points` in the example."""
    return ((seed * 31 + si * 53 + i * 17) % 60) + 10


def pie_value(seed: int, i: int) -> int:
    """`pie_points` in the example."""
    return ((seed * 13 + i * 41) % 50) + 5


def bar_mark(si: int, q: int, seed: int = 1) -> str:
    return f"{SERIES[si]}, {QUARTERS[q]}: {quarter_value(seed, si, q)}"


#: Bars are painted category-major (every series of Q1, then Q2, ...).
BAR_ORDER = [bar_mark(si, q) for q in range(4) for si in range(3)]
#: Line points are painted series-major (Revenue Q1..Q4, then Cost, ...).
LINE_ORDER = [bar_mark(si, q) for si in range(3) for q in range(4)]
PIE_TOTAL = sum(pie_value(1, i) for i in range(5))


def pie_mark(i: int) -> str:
    """What `hit::mark_description` makes of a slice of the example's pie,
    whose one series is unnamed (`ChartModel::from_points`)."""
    return f", {PIE_LABELS[i]}: {pie_value(1, i)}"


def pie_share(i: int) -> str:
    return f"{pie_value(1, i) / PIE_TOTAL * 100:.0f}%"


# ---------------------------------------------------------------------------
# Checks
# ---------------------------------------------------------------------------


def on_mark(name: str) -> list:
    """Focus lands on the mark and Orca says it, once."""
    return [focused(role="panel", name=name), said(name), said_once(name)]


def spoken(act) -> list[str]:
    return [u.text for u in utterances(act.orca)]


def said_word(word: str, describe: str):
    def run(act):
        texts = spoken(act)
        return any(word.lower() in t.lower() for t in texts), [f"Orca said {texts!r}"]
    return custom(describe, run, needs_orca=True)


def mark_state(name: str, state: str):
    """After the act, the mark `name` carries `state`."""
    from reader_lib.checks import _walk

    def run(act):
        for node in _walk(act.tree):
            if node.get("role") == "panel" and node.get("name") == name:
                return state in node.get("states", []), [
                    f"[panel] {name!r} states={node.get('states')}"]
        return False, [f"no [panel] {name!r} in the tree after the act"]
    return custom(f"the mark {name!r} has state {state!r}", run, needs_tree=True)


def utterance_count_at_most(n: int, describe: str):
    def run(act):
        texts = spoken(act)
        return len(texts) <= n, [f"{len(texts)} utterances: {texts!r}"]
    return custom(describe, run, needs_orca=True)


def no_raw_float(describe: str):
    """No utterance carries a value with more than two decimals."""
    import re

    def run(act):
        texts = spoken(act)
        raw = [t for t in texts if re.search(r"\d\.\d{3,}", t)]
        return not raw, [f"Orca said {t!r}" for t in raw] or [f"Orca said {texts!r}"]
    return custom(describe, run, needs_orca=True)


# ---------------------------------------------------------------------------
# Scene setting
# ---------------------------------------------------------------------------


def tab_to_bar_chart(run):
    with run.act("Tab four times to the bar chart",
                 [focused(role="document frame", name="Bar chart: 3 series, 4 categories"),
                  said("Bar chart: 3 series, 4 categories")],
                 should="focus lands on the bar chart and the reader hears what it is"):
        run.key("Tab", "Tab", "Tab", "Tab")


# ---------------------------------------------------------------------------
# Scenarios
# ---------------------------------------------------------------------------


def bar_keys(run):
    """Arrow, End, Home, clamp and Enter through the bar chart's marks."""
    tab_to_bar_chart(run)
    with run.act("Right: first bar", on_mark(BAR_ORDER[0]),
                 should=f"the first datum is reached and the reader hears {BAR_ORDER[0]!r} once"):
        run.key("Right")
    with run.act("Right: second bar", on_mark(BAR_ORDER[1]),
                 should=f"the reader hears {BAR_ORDER[1]!r} once"):
        run.key("Right")
    with run.act("Right: third bar", on_mark(BAR_ORDER[2]),
                 should=f"the reader hears {BAR_ORDER[2]!r} once"):
        run.key("Right")
    with run.act("End: last bar", on_mark(BAR_ORDER[-1]),
                 should=f"the last datum is reached and the reader hears {BAR_ORDER[-1]!r} once"):
        run.key("End")
    with run.act("Home: first bar", on_mark(BAR_ORDER[0]),
                 should=f"the first datum is reached again, {BAR_ORDER[0]!r} said once"):
        run.key("Home")
    with run.act("Left at the first bar (clamps)",
                 [no_event("object:state-changed:focused"),
                  not_said(BAR_ORDER[1])],
                 should="nothing moves: the traversal clamps at the first datum"):
        run.key("Left")
    with run.act("Enter selects the reached bar",
                 [mark_state(BAR_ORDER[0], "selected"),
                  in_tree(role="label", name_contains="Selected: Revenue"),
                  said_word("selected", "the reader hears that the datum is selected")],
                 should="the datum is selected: its node says so and the reader hears it",
                 tree=True):
        run.key("Return")


def bar_legend(run):
    """The interactive legend: Space on a row, and a screen reader's own
    activation of a row."""
    with run.act("Tab five times to the Revenue legend row",
                 [focused(role="check box", name="Revenue"),
                  said("Revenue check box checked")],
                 should="focus lands on the legend's first row, a checked check box"):
        run.key("Tab", "Tab", "Tab", "Tab", "Tab")
    with run.act("Space hides Revenue",
                 [event("object:state-changed:checked", role="check box",
                        name_contains="Revenue", detail1=0),
                  said_word("not checked", "the reader hears Revenue is now not checked"),
                  not_in_tree(role="panel", name_contains="Revenue,"),
                  in_tree(role="document frame", name_contains="Bar chart")],
                 should="the Revenue series leaves the plot and the row says not checked",
                 tree=True):
        run.key("space")
    with run.act("Space shows Revenue again",
                 [event("object:state-changed:checked", role="check box",
                        name_contains="Revenue", detail1=1),
                  said_word("checked", "the reader hears Revenue is checked again"),
                  in_tree(role="panel", name=BAR_ORDER[0])],
                 should="the Revenue series is back and the row says checked", tree=True):
        run.key("space")
    with run.act("Shift+Tab back to the chart, Right: first bar (re-shown)",
                 [focused(role="panel", name=BAR_ORDER[0]), said(BAR_ORDER[0])],
                 should=f"the reader hears {BAR_ORDER[0]!r}, a datum of the series shown again"):
        run.key("Shift+Tab", "Right")
    with run.act("Right: a Cost bar (never hidden)", on_mark(BAR_ORDER[1]),
                 should=f"the reader hears {BAR_ORDER[1]!r}"):
        run.key("Right")
    with run.act("Right: a Profit bar (never hidden)",
                 on_mark(BAR_ORDER[2]), should=f"the reader hears {BAR_ORDER[2]!r}"):
        run.key("Right")
    with run.act("Right: Revenue Q2 (re-shown)", on_mark(BAR_ORDER[3]),
                 should=f"the reader hears {BAR_ORDER[3]!r}"):
        run.key("Right")
    with run.act("Tab to the Revenue legend row again",
                 [focused(role="check box", name="Revenue"), said("Revenue")],
                 should="focus returns to the legend row"):
        run.key("Tab")
    failure: dict = {}

    def action_ran(act):
        if "error" in failure:
            return False, [f"run.action failed: {failure['error']}"]
        return True, ["the AT-SPI action was done"]

    with run.act("AT-SPI click on the Cost legend row",
                 [custom("the row offers an action a screen reader can activate", action_ran),
                  event("object:state-changed:checked", role="check box",
                        name_contains="Cost", detail1=0)],
                 should="a screen reader's own activation toggles the series, as a click does",
                 tree=True):
        try:
            run.action("click", role="check box", name="Cost")
        except RunError as exc:
            failure["error"] = str(exc)
            run.note(f"AT-SPI click on the Cost legend row: {exc}")


def mark_click(run):
    """A screen reader's activation of a mark it reached by review, with
    focus elsewhere."""
    target = bar_mark(1, 1)
    with run.act(f"AT-SPI click on the mark {target!r}",
                 [mark_state(target, "selected"),
                  in_tree(role="label", name_contains="Selected: Cost"),
                  said_word("selected", "the reader hears the datum is now selected")],
                 should="the datum is selected, as a click on it does, and the reader "
                        "learns it", tree=True):
        run.action("click", role="panel", name=target)
    with run.act("AT-SPI click on Clear selection",
                 [in_tree(role="label", name="Click a bar or point to select it.")],
                 should="the selection is cleared", tree=True):
        run.action("click", role="push button", name="Clear selection")


def style_toggle(run):
    """The ChartStyle toggle (a SegmentedControl) and the chart behind it."""
    with run.act("Tab three times to the chart style toggle",
                 [focused(role="radio button", name="Default"),
                  said("Default"),
                  not_said("not selected")],
                 should="focus lands on the style toggle and the reader hears the "
                        "current style, selected"):
        run.key("Tab", "Tab", "Tab")
    with run.act("Right: Gradient theme",
                 [focused(role="radio button", name="Gradient theme"),
                  said("Gradient theme"),
                  not_said("not selected"),
                  in_tree(role="radio button", name="Gradient theme", state="checked")],
                 should="the gradient style is chosen and the reader hears it selected",
                 tree=True):
        run.key("Right")
    with run.act("Tab to the gradient bar chart",
                 [focused(role="document frame", name="Bar chart: 3 series, 4 categories"),
                  said("Bar chart: 3 series, 4 categories")],
                 should="focus lands on the re-styled chart, which reads the same"):
        run.key("Tab")
    with run.act("Right: first bar of the gradient chart", on_mark(BAR_ORDER[0]),
                 should=f"the reader hears {BAR_ORDER[0]!r} once"):
        run.key("Right")
    with run.act("Shift+Tab to the style toggle, Left: Default again",
                 [focused(role="radio button", name="Default"), said("Default")],
                 should="the default style is back and the reader hears Default"):
        run.key("Shift+Tab", "Left")
    with run.act("Tab to the default bar chart again (shown for the second time)",
                 [focused(role="document frame", name="Bar chart: 3 series, 4 categories"),
                  said("Bar chart: 3 series, 4 categories")],
                 should="focus lands on the chart shown again, and the reader hears it"):
        run.key("Tab")
    with run.act("Right: first bar of the chart shown again", on_mark(BAR_ORDER[0]),
                 should=f"the reader hears {BAR_ORDER[0]!r} once"):
        run.key("Right")
    with run.act("Right: second bar of the chart shown again", on_mark(BAR_ORDER[1]),
                 should=f"the reader hears {BAR_ORDER[1]!r} once"):
        run.key("Right")


def lines(run):
    """The line chart: chart kind to Lines, then its points."""
    with run.act("Tab twice to the chart kind", [focused(role="radio button", name="Bars")],
                 should="focus lands on the chart kind"):
        run.key("Tab", "Tab")
    with run.act("Right: Lines",
                 [focused(role="radio button", name="Lines"), said("Lines"),
                  not_said("not selected")],
                 should="the line chart replaces the bars and the reader hears Lines, selected"):
        run.key("Right")
    with run.act("Tab twice to the line chart",
                 [focused(role="document frame", name_contains="Line chart"),
                  said("Line chart: 3 series, 4 points")],
                 should="focus lands on the line chart"):
        run.key("Tab", "Tab")
    with run.act("Right: first point", on_mark(LINE_ORDER[0]),
                 should=f"the reader hears {LINE_ORDER[0]!r} once"):
        run.key("Right")
    with run.act("Right: second point", on_mark(LINE_ORDER[1]),
                 should=f"the reader hears {LINE_ORDER[1]!r} once"):
        run.key("Right")
    with run.act("End: last point", on_mark(LINE_ORDER[-1]),
                 should=f"the reader hears {LINE_ORDER[-1]!r} once"):
        run.key("End")
    with run.act("Space selects the reached point",
                 [mark_state(LINE_ORDER[-1], "selected"),
                  in_tree(role="label", name_contains="Selected: Profit"),
                  said_word("selected", "the reader hears the point is selected")],
                 should="the point is selected and the reader hears it", tree=True):
        run.key("space")
    with run.act("Shift+Tab twice to the chart kind, Left: Bars again",
                 [focused(role="radio button", name="Bars"), said("Bars")],
                 should="the bar chart is shown again and the reader hears Bars"):
        run.key("Shift+Tab", "Shift+Tab", "Left")
    with run.act("Tab twice to the bar chart (shown for the second time)",
                 [focused(role="document frame", name="Bar chart: 3 series, 4 categories"),
                  said("Bar chart: 3 series, 4 categories")],
                 should="focus lands on the bar chart shown again, and the reader hears it"):
        run.key("Tab", "Tab")
    with run.act("Right: first bar of the chart shown again", on_mark(BAR_ORDER[0]),
                 should=f"the reader hears {BAR_ORDER[0]!r} once"):
        run.key("Right")
    with run.act("Right: second bar of the chart shown again", on_mark(BAR_ORDER[1]),
                 should=f"the reader hears {BAR_ORDER[1]!r} once"):
        run.key("Right")


def donut(run):
    """The donut: chart kind to Donut, then its slices, then a selection that
    the centre slot shows."""
    with run.act("Tab twice to the chart kind", [focused(role="radio button", name="Bars")],
                 should="focus lands on the chart kind"):
        run.key("Tab", "Tab")
    with run.act("End: Donut",
                 [focused(role="radio button", name="Donut"), said("Donut")],
                 should="the donut replaces the bars"):
        run.key("End")
    with run.act("Tab twice to the donut",
                 [focused(role="document frame", name_contains="Pie chart"),
                  said("Pie chart: 5 slices")],
                 should="focus lands on the donut", tree=True):
        run.key("Tab", "Tab")
    first = pie_mark(0)
    with run.act("Right: first slice",
                 [focused(role="panel", name_contains=f"{PIE_LABELS[0]}: "),
                  said(f"{PIE_LABELS[0]}: {pie_value(1, 0)}"),
                  custom("the slice's name does not start with a stray separator",
                         lambda act: (not any(t.lstrip().startswith(",") for t in spoken(act)),
                                      [f"Orca said {spoken(act)!r}"]), needs_orca=True),
                  said_word("%", f"the reader hears the slice's share ({pie_share(0)}), "
                                 "which the tooltip and the labels show")],
                 should=f"the reader hears the slice, its value and its share ({first!r} is "
                        "what the source makes)"):
        run.key("Right")
    with run.act("Right: second slice",
                 [focused(role="panel", name_contains=f"{PIE_LABELS[1]}: "),
                  said(f"{PIE_LABELS[1]}: {pie_value(1, 1)}")],
                 should="the reader hears the next slice"):
        run.key("Right")
    with run.act("Enter selects the slice; the centre slot shows it",
                 [mark_state(pie_mark(1), "selected"),
                  in_tree(role="label", name=PIE_LABELS[1]),
                  in_tree(role="label", name=pie_share(1)),
                  said_word("selected", "the reader hears the slice is selected")],
                 should="the slice is selected and the reader learns it (the centre slot "
                        f"now reads {PIE_LABELS[1]!r} / {pie_share(1)!r})", tree=True):
        run.key("Return")


def live(run):
    """The live strip chart: a reader resting on the chart and on a point while
    samples arrive, then pausing the feed and resting again."""
    with run.act("Shift+Tab twice to the raw live chart",
                 [focused(role="document frame", name_contains="Line chart: 1 series")],
                 should="focus lands on the raw (windowed) live chart"):
        run.key("Shift+Tab", "Shift+Tab")
    with run.act("rest on the chart for 3 s while samples arrive",
                 [utterance_count_at_most(1, "the reader, resting on the chart, is not "
                                             "read its new point count every 600 ms")],
                 should="the reader keeps their place; the chart does not speak every tick",
                 record=3.0):
        run.wait(0.1)
    with run.act("End: the newest sample",
                 [focused(role="panel", name_contains="Windowed, "),
                  said("Windowed, "),
                  no_raw_float("the value is said as the axis shows it, not as a raw float")],
                 should="the reader hears the newest sample"):
        run.key("End")
    with run.act("rest on the point for 3 s while samples arrive",
                 [utterance_count_at_most(1, "the reader, resting on a point, is not "
                                             "read a new sample every 600 ms"),
                  no_event("object:property-change:accessible-name", role="panel",
                           name_contains="Windowed, ")],
                 should="the reader keeps their place: the datum under focus stays the "
                        "datum it was, and is not re-read every tick", record=3.0, tree=True):
        run.wait(0.1)
    with run.act("Shift+Tab three times to the feed",
                 [focused(role="radio button", name="Running")],
                 should="focus lands on the feed toggle"):
        run.key("Shift+Tab", "Shift+Tab", "Shift+Tab")
    with run.act("Right: Paused",
                 [focused(role="radio button", name="Paused"), said("Paused")],
                 should="the feed pauses and the reader hears Paused, selected"):
        run.key("Right")
    with run.act("after pausing, the charts are quiet for 3 s",
                 [no_event("object:property-change:accessible-name", role="document frame"),
                  no_event("object:children-changed", role="document frame")],
                 should="no sample arrives once the feed is paused", record=3.0):
        run.wait(0.1)
    with run.act("Tab three times back to the raw chart, End",
                 [focused(role="panel", name_contains="Windowed, "), said("Windowed, ")],
                 should="the reader reaches the newest sample of the paused chart"):
        run.key("Tab", "Tab", "Tab", "End")
    with run.act("rest on the point of the paused chart for 3 s",
                 [utterance_count_at_most(0, "nothing is said while the feed is paused")],
                 should="silence: nothing changes", record=3.0):
        run.wait(0.1)


SCENARIOS = [
    Scenario("charts-bar-keys", "chart-demo", bar_keys,
             "arrows / End / Home / Enter across the bar chart's marks"),
    Scenario("charts-bar-legend", "chart-demo", bar_legend,
             "the interactive legend: Space, and an AT-SPI activation"),
    Scenario("charts-mark-click", "chart-demo", mark_click,
             "a screen reader's activation of a bar, then Clear selection"),
    Scenario("charts-style-toggle", "chart-demo", style_toggle,
             "the Default / Gradient theme ChartStyle toggle and the chart after it"),
    Scenario("charts-lines", "chart-demo", lines,
             "chart kind to Lines, then the line chart's points"),
    Scenario("charts-donut", "chart-demo", donut,
             "chart kind to Donut, then its slices and the centre slot"),
    Scenario("charts-live", "chart-demo", live,
             "the live strip chart: resting on a point while samples arrive, then Pause"),
]
