# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Verifier's scenarios for chart-demo (`examples/chart_demo/src/main.rs`).

The sweep's scenarios (`charts.py`) run with the live strip charts feeding
every 600 ms, which keeps ~150 AT-SPI events a second on the bus and Orca
permanently behind. These scenarios PAUSE the feed first (the "Running /
Paused" SegmentedControl, five Shift+Tabs from launch), so the bar chart,
legend and donut are measured with an idle Orca: what remains is the chart's
own behaviour, not the backlog's.

`verify-charts-live-full` does the opposite: it lets the raw strip chart's
window fill (24 samples, ~15 s) before resting on it, to separate "the chart's
name changes while the window fills" from "the name changes forever", and
rests on the oldest point and on the rollup chart.
"""

from reader_lib.checks import (custom, event, focused, in_tree, no_event, not_in_tree,
                               said, said_once)
from reader_lib.orca import seconds, utterances
from reader_lib.run import RunError
from reader_lib.scenario import Scenario

SERIES = ["Revenue", "Cost", "Profit"]
QUARTERS = ["Q1", "Q2", "Q3", "Q4"]
PIE_LABELS = ["Storage", "Apps", "System", "Cache", "Free"]
BAR_NAME = "Bar chart: 3 series, 4 categories"


def quarter_value(seed: int, si: int, i: int) -> int:
    return ((seed * 31 + si * 53 + i * 17) % 60) + 10


def pie_value(seed: int, i: int) -> int:
    return ((seed * 13 + i * 41) % 50) + 5


def bar_mark(si: int, q: int, seed: int = 1) -> str:
    return f"{SERIES[si]}, {QUARTERS[q]}: {quarter_value(seed, si, q)}"


BAR_ORDER = [bar_mark(si, q) for q in range(4) for si in range(3)]


# ---------------------------------------------------------------------------
# Checks
# ---------------------------------------------------------------------------


def spoken(act) -> list[str]:
    return [u.text for u in utterances(act.orca)]


def count_said(text: str, n: int, describe: str):
    """Exactly `n` utterances contain `text` (and report cut flags)."""
    def run(act):
        heard = [u for u in utterances(act.orca) if text.lower() in u.text.lower()]
        ev = [f"Orca said{' (cut)' if u.cut else ''} at {u.stamp}: {u.text!r}" for u in heard]
        return len(heard) == n, ev or [f"never said {text!r}; all: {spoken(act)!r}"]
    return custom(describe, run, needs_orca=True)


def said_word(word: str, describe: str):
    def run(act):
        texts = spoken(act)
        return any(word.lower() in t.lower() for t in texts), [f"Orca said {texts!r}"]
    return custom(describe, run, needs_orca=True)


def mark_state(name: str, state: str):
    from reader_lib.checks import _walk

    def run(act):
        for node in _walk(act.tree):
            if node.get("role") == "panel" and node.get("name") == name:
                return state in node.get("states", []), [
                    f"[panel] {name!r} states={node.get('states')}"]
        return False, [f"no [panel] {name!r} in the tree after the act"]
    return custom(f"the mark {name!r} has state {state!r}", run, needs_tree=True)


def chart_name_and_marks(prefix: str):
    """Report the chart's name and its mark children, for the record."""
    from reader_lib.checks import _walk

    def run(act):
        for node in _walk(act.tree):
            if node.get("role") == "document frame" and (node.get("name") or "").startswith(prefix):
                kids = [c.get("name") for c in node.get("children", []) if c.get("role") == "panel"]
                return True, [f"[document frame] {node.get('name')!r}: {len(kids)} marks {kids!r}"]
        return False, ["no such chart"]
    return custom(f"record the {prefix!r} chart's name and marks", run, needs_tree=True)


def in_window_speech(describe: str, at_most: int):
    """Utterances whose Orca stamp falls inside the act's own wall window
    (start..end), not the catch-up window, and how many there were."""
    def run(act):
        s, e = seconds(act.start_wall), seconds(act.end_wall)
        inside = [u for u in utterances(act.orca) if s <= seconds(u.stamp) <= e]
        total = utterances(act.orca)
        ev = [f"{len(inside)} utterances inside the act's own {e - s:.1f} s window "
              f"({act.start_wall}..{act.end_wall}); {len(total)} in Orca's whole window "
              f"({act.orca_start}..{act.orca_end})"]
        ev += [f"  {u.stamp} {u.text!r}" for u in inside[:40]]
        return len(inside) <= at_most, ev
    return custom(describe, run, needs_orca=True)


def name_changes_on_focus(describe: str):
    """Count accessible-name / value changes on the node focus was last on."""
    def run(act):
        from reader_lib.checks import _is_focus, _focus_node
        moves = [e for e in act.history if _is_focus(e)]
        if not moves:
            return False, ["no focus in history"]
        path = _focus_node(moves[-1]).get("path")
        names = [e for e in act.events if e["type"] == "object:property-change:accessible-name"
                 and e.get("source", {}).get("path") == path]
        values = [e for e in act.events if e["type"] == "object:property-change:accessible-value"
                  and e.get("source", {}).get("path") == path]
        ev = [f"focused path {path}: {len(names)} name changes, {len(values)} value changes "
              f"in {act.end_mono - act.start_mono:.1f} s"]
        ev += [f"  {act.rel_ms(e):+.0f} ms name -> {e.get('text')!r}" for e in names[:12]]
        return not names, ev
    return custom(describe, run)


def events_per_second(describe: str):
    def run(act):
        dur = act.end_mono - act.start_mono
        n = len(act.events)
        by: dict = {}
        for e in act.events:
            by[e["type"]] = by.get(e["type"], 0) + 1
        top = sorted(by.items(), key=lambda kv: -kv[1])[:6]
        return True, [f"{n} events in {dur:.1f} s = {n / dur:.1f}/s", f"by type: {top}"]
    return custom(describe, run)


# ---------------------------------------------------------------------------
# Scene setting
# ---------------------------------------------------------------------------


def pause_feed(run):
    with run.act("Shift+Tab five times to the feed control",
                 [focused(role="radio button", name="Running")],
                 should="focus lands on the feed's Running segment"):
        run.key("Shift+Tab", "Shift+Tab", "Shift+Tab", "Shift+Tab", "Shift+Tab")
    with run.act("Right: Paused", [focused(role="radio button", name="Paused")],
                 should="the feed pauses"):
        run.key("Right")
    with run.act("quiet for 2 s after pausing",
                 [no_event("object:property-change:accessible-name", role="document frame"),
                  no_event("object:property-change:accessible-value", role="panel"),
                  events_per_second("event rate while paused")],
                 should="no sample arrives: the rest of the run meets an idle Orca", record=2.0):
        run.wait(0.1)


# ---------------------------------------------------------------------------
# Scenarios
# ---------------------------------------------------------------------------


def quiet_bar(run):
    pause_feed(run)
    with run.act("Shift+Tab eight times to the bar chart",
                 [focused(role="document frame", name=BAR_NAME), said(BAR_NAME)],
                 should="focus lands on the bar chart"):
        run.key(*["Shift+Tab"] * 8)
    with run.act("Right: first bar (idle Orca)",
                 [focused(role="panel", name=BAR_ORDER[0]),
                  count_said(BAR_ORDER[0], 1, f"Orca says {BAR_ORDER[0]!r} exactly once")],
                 should="the first datum is heard once"):
        run.key("Right")
    with run.act("Right: second bar (idle Orca)",
                 [focused(role="panel", name=BAR_ORDER[1]),
                  count_said(BAR_ORDER[1], 1, f"Orca says {BAR_ORDER[1]!r} exactly once")],
                 should="the next datum is heard once"):
        run.key("Right")
    with run.act("Right: third bar (idle Orca)",
                 [focused(role="panel", name=BAR_ORDER[2]),
                  count_said(BAR_ORDER[2], 1, f"Orca says {BAR_ORDER[2]!r} exactly once")],
                 should="the next datum is heard once"):
        run.key("Right")
    with run.act("Enter selects the reached bar",
                 [mark_state(BAR_ORDER[2], "selected"),
                  mark_state(BAR_ORDER[2], "selectable"),
                  in_tree(role="label", name_contains="Selected: Profit"),
                  event("object:state-changed:selected"),
                  said_word("selected", "the reader hears that the datum is selected")],
                 should="the datum is selected and the reader learns it", tree=True):
        run.key("Return")
    with run.act("Space on the next bar selects it",
                 [mark_state(BAR_ORDER[3], "selected"),
                  said_word("selected", "the reader hears that the datum is selected")],
                 should="Space commits too, and the reader learns it", tree=True):
        run.key("Right", "space")
    target = bar_mark(1, 1)
    with run.act(f"AT-SPI click on the mark {target!r}",
                 [mark_state(target, "selected"),
                  in_tree(role="label", name_contains="Selected: Cost"),
                  said_word("selected", "the reader hears the datum is now selected")],
                 should="a screen reader's activation selects the datum and says so", tree=True):
        run.action("click", role="panel", name=target)
    with run.act("Tab to the Revenue legend row",
                 [focused(role="check box", name="Revenue"), said("Revenue")],
                 should="focus lands on the legend's first row"):
        run.key("Tab")
    with run.act("Space hides Revenue",
                 [event("object:state-changed:checked", role="check box",
                        name_contains="Revenue", detail1=0),
                  said_word("not checked", "the reader hears Revenue is now not checked"),
                  not_in_tree(role="panel", name_contains="Revenue,"),
                  chart_name_and_marks("Bar chart"),
                  in_tree(role="document frame", name=BAR_NAME)],
                 should="the Revenue series leaves the plot and the row says not checked; "
                        "the chart's name still says 3 series (charts-11)", tree=True):
        run.key("space")
    failure: dict = {}

    def action_ran(act):
        if "error" in failure:
            return False, [f"run.action failed: {failure['error']}"]
        return True, ["the AT-SPI action was done"]

    with run.act("AT-SPI click on the Cost legend row",
                 [custom("the row offers an action a screen reader can activate", action_ran),
                  event("object:state-changed:checked", role="check box",
                        name_contains="Cost", detail1=0)],
                 should="a screen reader's own activation toggles the series", tree=True):
        try:
            run.action("click", role="check box", name="Cost")
        except RunError as exc:
            failure["error"] = str(exc)
    with run.act("AT-SPI action 0 on the Cost legend row (whatever it offers)",
                 [custom("the row offers any action", action_ran)],
                 should="any action at all", tree=False):
        failure.clear()
        try:
            run.action(0, role="check box", name="Cost")
        except RunError as exc:
            failure["error"] = str(exc)
    with run.act("Space shows Revenue again",
                 [event("object:state-changed:checked", role="check box",
                        name_contains="Revenue", detail1=1),
                  in_tree(role="panel", name=BAR_ORDER[0])],
                 should="the series is back", tree=True):
        run.key("space")
    with run.act("Shift+Tab back to the chart",
                 [focused(role="document frame", name=BAR_NAME), said(BAR_NAME)],
                 should="focus returns to the chart"):
        run.key("Shift+Tab")
    with run.act("Right: first bar on the second entry (idle Orca)",
                 [focused(role="panel", name=BAR_ORDER[0]),
                  count_said(BAR_ORDER[0], 1, f"Orca says {BAR_ORDER[0]!r} exactly once")],
                 should="the datum is heard once (on main the announcer's second message "
                        "comes from a defunct node, K2)"):
        run.key("Right")


def quiet_donut(run):
    pause_feed(run)
    with run.act("Tab six times to the chart kind",
                 [focused(role="radio button", name="Bars")],
                 should="focus wraps to the chart kind"):
        run.key(*["Tab"] * 6)
    with run.act("End: Donut", [focused(role="radio button", name="Donut")],
                 should="the donut replaces the bars"):
        run.key("End")
    with run.act("Tab twice to the donut",
                 [focused(role="document frame", name_contains="Pie chart"),
                  said("Pie chart: 5 slices")],
                 should="focus lands on the donut", tree=True):
        run.key("Tab", "Tab")
    with run.act("Right: first slice (idle Orca)",
                 [focused(role="panel", name_contains="Storage: "),
                  count_said("Storage: ", 1, "Orca says the first slice exactly once"),
                  custom("no utterance starts with a stray ', '",
                         lambda act: (not any(t.lstrip().startswith(",") for t in spoken(act)),
                                      [f"Orca said {spoken(act)!r}"]), needs_orca=True),
                  said_word("%", "the reader hears the slice's share")],
                 should="the slice, value and share, once"):
        run.key("Right")
    with run.act("Right: second slice (idle Orca)",
                 [focused(role="panel", name_contains="Apps: "),
                  count_said("Apps: ", 1, "Orca says the second slice exactly once")],
                 should="the next slice, once"):
        run.key("Right")
    with run.act("Enter selects the slice",
                 [mark_state(f", {PIE_LABELS[1]}: {pie_value(1, 1)}", "selected"),
                  in_tree(role="label", name=PIE_LABELS[1]),
                  said_word("selected", "the reader hears the slice is selected")],
                 should="the slice is selected and the reader learns it", tree=True):
        run.key("Return")


def live_full(run):
    run.note("waiting 14 s so the raw window (24 samples at 600 ms) is full")
    run.wait(14.0)
    with run.act("Shift+Tab twice to the raw live chart (window full)",
                 [focused(role="document frame", name_contains="Line chart: 1 series")],
                 should="focus lands on the raw live chart", tree=True):
        run.key("Shift+Tab", "Shift+Tab")
    with run.act("rest on the full raw chart for 3 s",
                 [no_event("object:property-change:accessible-name", role="document frame",
                           name_contains="Line chart: 1 series, 24"),
                  in_window_speech("at most one utterance while resting on the full chart", 1),
                  events_per_second("event rate while resting")],
                 should="once the window is full the chart's own name no longer changes",
                 record=3.0, tree=True):
        run.wait(0.1)
    with run.act("Home: the oldest sample in the window",
                 [focused(role="panel", name_contains="Windowed, ")],
                 should="the reader reaches the oldest windowed sample"):
        run.key("Home")
    with run.act("rest on the oldest point for 3 s",
                 [name_changes_on_focus("the focused point keeps its name (its datum)"),
                  in_window_speech("at most one utterance while resting on a point", 1)],
                 should="the datum under focus stays the datum it was", record=3.0, tree=True):
        run.wait(0.1)
    with run.act("Tab to the rollup chart",
                 [focused(role="document frame", name_contains="Line chart: 1 series")],
                 should="focus lands on the rollup chart"):
        run.key("Tab")
    with run.act("rest on the rollup chart for 6 s",
                 [no_event("object:property-change:accessible-name", role="document frame"),
                  in_window_speech("at most one utterance while resting on the rollup chart",
                                   1)],
                 should="the reader resting on the rollup chart is not read a new point "
                        "count every bucket", record=6.0, tree=True):
        run.wait(0.1)


def reshown_visited(run):
    """Nodes the reader has visited, taken out of the tree, and shown again:
    a legend-hidden series, a Switcher page (the ChartStyle toggle), and the
    chart-kind control scrolled out of the ScrollArea's clip (AccessKit's
    `common_filter` drops a clipped child whose neighbours are clipped too).
    Each comes back with the same AT-SPI path."""
    pause_feed(run)
    with run.act("Shift+Tab eight times to the bar chart",
                 [focused(role="document frame", name=BAR_NAME), said(BAR_NAME)],
                 should="focus lands on the bar chart"):
        run.key(*["Shift+Tab"] * 8)
    with run.act("Right: Revenue Q1 (first visit)",
                 [focused(role="panel", name=BAR_ORDER[0]), said(BAR_ORDER[0])],
                 should="the reader hears the first bar"):
        run.key("Right")
    with run.act("Right: Cost Q1 (first visit)",
                 [focused(role="panel", name=BAR_ORDER[1]), said(BAR_ORDER[1])],
                 should="the reader hears the second bar"):
        run.key("Right")
    with run.act("Tab to the Revenue legend row, Space hides Revenue",
                 [event("object:state-changed:checked", role="check box",
                        name_contains="Revenue", detail1=0),
                  event("object:state-changed:defunct", role="panel", name_contains="Revenue, Q1")],
                 should="the Revenue marks leave the tree"):
        run.key("Tab", "space")
    with run.act("Space shows Revenue again",
                 [event("object:children-changed:add", role="document frame")],
                 should="the Revenue marks come back"):
        run.key("space")
    with run.act("Shift+Tab to the chart, Right: Revenue Q1 (visited, hidden, shown again)",
                 [focused(role="panel", name=BAR_ORDER[0]), said(BAR_ORDER[0])],
                 should="the reader hears the re-shown bar they had visited before"):
        run.key("Shift+Tab", "Right")
    with run.act("Right: Cost Q1 (never hidden)",
                 [focused(role="panel", name=BAR_ORDER[1]), said(BAR_ORDER[1])],
                 should="control: a bar that never left the tree"):
        run.key("Right")
    with run.act("Shift+Tab to the style toggle, Right: Gradient theme",
                 [focused(role="radio button", name="Gradient theme"), said("Gradient theme"),
                  event("object:state-changed:defunct", role="document frame",
                        name_contains="Bar chart")],
                 should="the default chart (visited) leaves the tree"):
        run.key("Shift+Tab", "Right")
    with run.act("Tab to the gradient chart (never visited)",
                 [focused(role="document frame", name=BAR_NAME), said(BAR_NAME)],
                 should="control: a chart the reader never visited"):
        run.key("Tab")
    with run.act("Shift+Tab to the style toggle, Left: Default",
                 [focused(role="radio button", name="Default"), said("Default")],
                 should="the default chart comes back"):
        run.key("Shift+Tab", "Left")
    with run.act("Tab to the default chart (visited, hidden, shown again)",
                 [focused(role="document frame", name=BAR_NAME), said(BAR_NAME)],
                 should="the reader hears the chart they had visited before"):
        run.key("Tab")
    with run.act("Right: Revenue Q1 of the default chart shown again",
                 [focused(role="panel", name=BAR_ORDER[0]), said(BAR_ORDER[0])],
                 should="the reader hears the bar"):
        run.key("Right")
    with run.act("Shift+Tab twice to the chart kind (scrolled out and back)",
                 [focused(role="radio button", name="Bars"), said("Bars")],
                 should="the reader hears the chart kind control"):
        run.key("Shift+Tab", "Shift+Tab")
    with run.act("Right: Lines",
                 [focused(role="radio button", name="Lines"), said("Lines")],
                 should="the reader hears Lines"):
        run.key("Right")


SCENARIOS = [
    Scenario("verify-charts-quiet-bar", "chart-demo", quiet_bar,
             "feed paused; bar chart traversal, selection, legend, second entry"),
    Scenario("verify-charts-quiet-donut", "chart-demo", quiet_donut,
             "feed paused; donut slices and selection"),
    Scenario("verify-charts-reshown-visited", "chart-demo", reshown_visited,
             "feed paused; visited nodes that leave the tree and come back"),
    Scenario("verify-charts-live-full", "chart-demo", live_full,
             "live feed running; rest on the full raw chart, its oldest point, the rollup"),
]
