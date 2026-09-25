# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""The examples the sweep had not covered, as a screen reader meets them.

* `simple-button` (`examples/simple_button/src/main.rs`): one Filled `Button`
  "Click Me" with a plain tooltip, whose handler prints "Button clicked!".
  The window also carries the debug inspector (F12).
* `automation_bridge_smoke` (`examples/automation_bridge_smoke/src/main.rs`):
  a "Save" `Button` with no handler and an `InputProbe`, a `Role::Button`
  named "input-probe" that publishes the last input it saw as its value and
  takes no focus. Without `--serve` an in-process client drives the bridge
  and exits the process; every scenario here passes `--serve`, which keeps
  the window up and runs no client.
* `telemetry-plausible` (`examples/telemetry_plausible/src/main.rs`): a
  toolbar with the `ThemeSwitcher`, three buttons that each send an intent
  (printed as "intent dispatched: app.demo.<x>"), and `PrivacySettings`, the
  consent panel (`crates/teksilo-widgets/src/privacy_settings.rs`). Every run
  sets `PLAUSIBLE_ENDPOINT` to a closed port on the loopback address, so
  whatever the adapter posts once consent is given is refused on this machine
  and nothing leaves it; the private session's configuration home is empty,
  so no saved endpoint override is read either. The privacy policy URL is a
  label, not a link, so nothing here can open a browser.
* `web-view-demo` (`examples/web_view_demo/src/main.rs`): two buttons that
  switch a `Switcher` between the `WebView` and a native panel, back,
  forward, reload, "Send to JS", "DevTools" and "Load example.com" buttons,
  the URL, a loading glyph and a status line. The private session has no
  `DISPLAY`, so the demo stays on Wayland, where wry cannot embed WebKitGTK
  (`build_as_child` wants an X11 parent) and the `WebView` goes to its error
  state (`crates/teksilo-webview/src/wry_backend.rs`, `fail`). The scenarios
  never press "Load example.com", which would load a page from the network
  on an engine that works.

`telemetry-teksilo` is not here, because it does not build. The root
manifest excludes it from the workspace, yet its own manifest inherits
`version`, `authors` and `teksilo` from a workspace, so cargo stops with
"failed to find a workspace root". Its adapter, `teksilo-analytics-native`,
also pins `teksilo-core` and `teksilo-telemetry` at 0.6.2 against the tree's
0.13.1, and finds `teksilo-collector-proto` three directories up, outside the
checkout.

`rest-telemetry-tabwalk` is the Tab walk of `telemetry-plausible` with the
same loopback endpoint; `reader.py tabwalk` would launch it with the example's
default endpoint, which is on this machine too.
"""

from __future__ import annotations

from contextlib import contextmanager

from reader_lib.checks import (_focus_node, _is_focus, _walk, announced, custom, focus_stays,
                               focused, in_tree, no_event, not_in_tree, said)
from reader_lib.orca import utterances
from reader_lib.scenario import Scenario, tab_walk_scenario

#: Where the Plausible adapter posts: a closed port on this machine.
LOCAL_ENDPOINT = {"PLAUSIBLE_ENDPOINT": "http://127.0.0.1:1/api/event"}


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


class Printed:
    """How many times the example printed a line, act by act.

    Checks are judged after the run, so the count is taken as each act starts
    and after it ends, and the check reads the two numbers it was left."""

    def __init__(self, run, text: str) -> None:
        self.run = run
        self.text = text
        self.marks: dict[str, list[int]] = {}

    def count(self) -> int:
        try:
            return self.run.app_log.read_text(errors="replace").count(self.text)
        except OSError:
            return 0

    def check(self, label: str, times: int):
        def judge(_act):
            before, after = self.marks.get(label, [0, 0])
            got = after - before
            return got == times, [f"{self.text!r} printed {got} time(s) in the act"]
        return custom(f"the example prints {self.text!r} {times} time(s)", judge)

    @contextmanager
    def act(self, label: str, expect=(), *, times: int = 1, **kw):
        with self.run.act(label, [*expect, self.check(label, times)], **kw) as act:
            self.marks[label] = [self.count(), self.count()]
            yield act
        self.marks[label][1] = self.count()


def scene(run, label: str, step, record: float = 1.0) -> None:
    """Set a scene inside an act of its own, judged by nothing."""
    with run.act(f"scene: {label}", should="scene setting, not judged", record=record):
        step()


def focus_sequence():
    """Every focus landing of the act, in order (a record, always passes)."""
    def run(act):
        moves = [_focus_node(e) for e in act.events if _is_focus(e)]
        return True, [" -> ".join(f"[{n.get('role')}] {n.get('name')!r}" for n in moves)
                      or "no focus change"]
    return custom("the focus landings of the act, in order (a record)", run)


def speech_record():
    """Everything Orca said in the act (a record, always passes)."""
    def run(act):
        return True, [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                      for u in utterances(act.orca)] or ["Orca said nothing"]
    return custom("what Orca said (a record)", run, needs_orca=True)


def node_states(role: str, name: str | None = None, *, name_contains: str | None = None,
                want: str | None = None, absent: str | None = None):
    """A node's AT-SPI states, actions and description after the act; passes
    when `want` is among the states and `absent` is not."""
    def run(act):
        node = next((n for n in _walk(act.tree) if n.get("role") == role
                     and (name is None or (n.get("name") or "") == name)
                     and (name_contains is None
                          or name_contains.lower() in (n.get("name") or "").lower())), None)
        if node is None:
            return False, [f"no [{role}] {name or name_contains!r} in the tree"]
        states = node.get("states", [])
        ok = (want is None or want in states) and (absent is None or absent not in states)
        return ok, [f"[{role}] {node.get('name')!r} states={states}",
                    f"actions={[a.get('name') for a in node.get('actions', [])]}",
                    f"description={node.get('description')!r}"]
    what = ", ".join(x for x in (f"is {want}" if want else "",
                                  f"is not {absent}" if absent else "") if x) or "(a record)"
    return custom(f"[{role}] {name or name_contains!r} {what}", run, needs_tree=True)


def focus_ends_on(role: str, name: str):
    """After the act focus is on a matching node: either the act moved it
    there, or it did not move and was there before."""
    def run(act):
        moves = [e for e in act.events if _is_focus(e)]
        if moves:
            node = _focus_node(moves[-1])
            how = "moved to"
        else:
            before = [e for e in act.history if _is_focus(e) and e["mono"] < act.start_mono]
            if not before:
                return False, ["focus never moved on the bus"]
            node = _focus_node(before[-1])
            how = "stayed on"
        ok = node.get("role") == role and (node.get("name") or "") == name
        return ok, [f"focus {how} [{node.get('role')}] {node.get('name')!r}"]
    return custom(f"focus ends on [{role}] {name!r}", run)


def launch_focus_on_control():
    """Where the launch left focus, read from the events before the act: a
    control, not the bare window."""
    def run(act):
        before = [e for e in act.history if _is_focus(e) and e["mono"] < act.start_mono]
        if not before:
            return False, ["no focus event at all before the act"]
        node = _focus_node(before[-1])
        return node.get("role") != "frame", [
            f"after launch focus is on [{node.get('role')}] {node.get('name')!r}"]
    return custom("the window opens with focus on a control, not on the bare frame", run)


def no_symbol_names():
    """No push button is named by a symbol alone (a glyph a speech synthesizer
    reads as its Unicode name, or skips)."""
    def run(act):
        bad = [n for n in _walk(act.tree) if n.get("role") == "push button"
               and (n.get("name") or "").strip()
               and not any(ch.isalnum() for ch in n.get("name") or "")]
        return not bad, [f"[push button] {n.get('name')!r}" for n in bad] or ["none"]
    return custom("every push button is named in words", run, needs_tree=True)


def unnamed_controls(within_role: str | None = None):
    """Every focusable node in the tree has a name (within the first node of
    `within_role` when given)."""
    def run(act):
        root = act.tree
        if within_role is not None:
            root = next((n for n in _walk(act.tree) if n.get("role") == within_role), None)
            if root is None:
                return False, [f"no [{within_role}] in the tree"]
        bad = [n for n in _walk(root) if "focusable" in n.get("states", [])
               and not (n.get("name") or "").strip() and n.get("role") != "frame"]
        return not bad, [f"unnamed focusable [{n.get('role')}] "
                         f"attrs={n.get('attributes')}" for n in bad] or ["none"]
    return custom("every focusable control has a name", run, needs_tree=True)


def web_view_tells_state():
    """The web view node says its page did not load: a busy state while
    loading, and something (a description, a name) once it failed."""
    def run(act):
        node = next((n for n in _walk(act.tree) if n.get("role") == "panel"
                     and "focusable" in n.get("states", [])), None)
        if node is None:
            return False, ["no focusable panel (the web view) in the tree"]
        desc = node.get("description") or ""
        name = node.get("name") or ""
        told = "busy" in node.get("states", []) or any(
            w in (desc + " " + name).lower() for w in ("error", "fail", "could not", "unavailable"))
        return told, [f"[panel] name={name!r} description={desc!r} states={node.get('states')}"]
    return custom("the web view tells a reader its page did not load", run, needs_tree=True)


def no_focus_on_frame():
    """Focus did not fall to the bare window in the act."""
    def run(act):
        moves = [_focus_node(e) for e in act.events if _is_focus(e)]
        frame = [n for n in moves if n.get("role") == "frame"]
        return not frame, [" -> ".join(f"[{n.get('role')}] {n.get('name')!r}" for n in moves)
                           or "no focus change"]
    return custom("focus does not fall to the window frame", run)


# ---------------------------------------------------------------------------
# simple-button
# ---------------------------------------------------------------------------


BUTTON = "Click Me"
BUTTON_TIP = "This is a simple button. Click it to see a message in the console."


def simple_button(run):
    clicks = Printed(run, "Button clicked!")
    run.wait_for(role="push button", name=BUTTON)
    with run.act("the window as launched, nothing pressed",
                 [in_tree(role="push button", name=BUTTON, description_contains=BUTTON_TIP),
                  launch_focus_on_control(), speech_record()],
                 should="the reader has the window's name and lands on the one button, which "
                        "carries its tooltip text as its description",
                 tree=True, record=0.5):
        pass
    with run.act("Tab to the button",
                 [focused(role="push button", name=BUTTON), said(f"{BUTTON} push button"),
                  said(BUTTON_TIP)],
                 should="focus moves to the button, and the reader says its name, its role "
                        "and the tooltip text a sighted user gets on hover",
                 record=3.0):
        run.key("Tab")
    with clicks.act("Space activates it",
                    [focus_stays(role="push button", name=BUTTON), speech_record()],
                    should="the handler runs once and focus stays on the button"):
        run.key("space")
    with clicks.act("Enter activates it",
                    [focus_stays(role="push button", name=BUTTON), speech_record()],
                    should="Enter is the other activation key of a push button"):
        run.key("Return")
    with clicks.act("AT-SPI click activates it",
                    [focus_stays(role="push button", name=BUTTON), speech_record()],
                    should="a screen reader's own activation reaches the handler"):
        run.action("click", role="push button", name=BUTTON)
    with run.act("Tab from the only control",
                 [no_event("object:state-changed:focused", role="frame"), speech_record()],
                 should="focus stays on the button (it is the only stop), nothing moves "
                        "to the bare window"):
        run.key("Tab")
    with run.act("Shift+Tab from the only control",
                 [no_event("object:state-changed:focused", role="frame"), speech_record()],
                 should="the same, backwards"):
        run.key("Shift+Tab")
    with run.act("the button kept after a pause with focus on it",
                 [in_tree(role="push button", name=BUTTON, description_contains=BUTTON_TIP),
                  speech_record()],
                 should="a tooltip that shows on keyboard focus is not re-announced over "
                        "and over, and the description is still there",
                 tree=True, record=3.0):
        pass


def simple_button_inspector(run):
    """The debug inspector (F12), which every debug build of the example
    installs: what a reader gets when they open and close it."""
    run.wait_for(role="push button", name=BUTTON)
    scene(run, "Tab to the button", lambda: run.key("Tab"))
    with run.act("F12 opens the inspector",
                 [focus_sequence(), speech_record(), unnamed_controls()],
                 should="the reader learns the inspector opened, and can reach it",
                 tree=True, record=3.0):
        run.key("F12")
    with run.act("Tab once in the inspector",
                 [focus_sequence(), speech_record()],
                 should="focus moves to a named control", tree=True, record=2.0):
        run.key("Tab")
    with run.act("F12 closes the inspector",
                 [focused(role="push button", name=BUTTON), no_focus_on_frame(),
                  focus_sequence(), speech_record()],
                 should="the inspector leaves the tree and focus goes back to the button it "
                        "was on before the reader went into the inspector",
                 tree=True, record=3.0):
        run.key("F12")


# ---------------------------------------------------------------------------
# automation_bridge_smoke --serve
# ---------------------------------------------------------------------------


def bridge_smoke(run):
    run.wait_for(role="push button", name="Save")
    with run.act("the window as launched",
                 [in_tree(role="push button", name="Save", state="focusable"),
                  node_states("push button", "input-probe", want="focusable"),
                  speech_record()],
                 should="each control a reader meets is one they can use",
                 tree=True, record=0.5):
        pass
    with run.act("Tab to Save", [focused(role="push button", name="Save"),
                                 said("Save push button")],
                 should="focus moves to Save and the reader says it"):
        run.key("Tab")
    with run.act("Space on Save", [focus_stays(role="push button", name="Save"),
                                   speech_record()],
                 should="nothing breaks; the example gives Save no handler"):
        run.key("space")
    with run.act("Tab from Save", [no_event("object:state-changed:focused", role="frame"),
                                   focus_sequence(), speech_record()],
                 should="focus stays on Save, the only Tab stop"):
        run.key("Tab")


# ---------------------------------------------------------------------------
# telemetry-plausible
# ---------------------------------------------------------------------------


SWITCH = "Anonymous usage metrics"


def _to_switch(run):
    run.wait_for(role="toggle button", name=SWITCH, timeout=30.0)
    run.grab_focus(role="toggle button", name=SWITCH)


def telemetry_switch(run):
    scene(run, "focus the consent switch", lambda: _to_switch(run))
    with run.act("Space turns the consent switch on",
                 [focused(role="toggle button", name=SWITCH), said("pressed"),
                  no_focus_on_frame(), focus_sequence(), speech_record(),
                  node_states("toggle button", SWITCH, want="pressed")],
                 should="the switch turns on, focus stays on it, and the reader hears it "
                        "is on (a sighted user sees the knob move)",
                 tree=True, record=3.0):
        run.key("space")
    with run.act("Tab from the switch",
                 [focused(role="push button", name="Reject all"), said("Reject all")],
                 should="Tab goes on to the next control after the switch, Reject all"):
        run.key("Tab")
    with run.act("Shift+Tab back onto the switch",
                 [focused(role="toggle button", name=SWITCH), said("pressed"),
                  said("Counts of which buttons")],
                 should="the reader hears the switch with the line that says what it shares, "
                        "which a sighted user reads beside it"):
        run.key("Shift+Tab")
    with run.act("Space turns it off again",
                 [focused(role="toggle button", name=SWITCH), said("not pressed"),
                  no_focus_on_frame(), focus_sequence(), speech_record(),
                  node_states("toggle button", SWITCH, absent="pressed")],
                 should="the switch turns off, focus stays on it, and the reader hears it",
                 tree=True, record=3.0):
        run.key("space")


def telemetry_accept_reject(run):
    run.wait_for(role="push button", name="Accept all", timeout=30.0)
    scene(run, "focus Accept all", lambda: run.grab_focus(role="push button", name="Accept all"))
    with run.act("Space on Accept all",
                 [focus_ends_on("push button", "Accept all"), no_focus_on_frame(),
                  focus_sequence(), speech_record(),
                  node_states("toggle button", SWITCH, want="pressed")],
                 should="consent is granted: the switch shows on; focus stays on Accept all, "
                        "and the reader is not left on the bare window",
                 tree=True, record=3.0):
        run.key("space")
    with run.act("Tab after Accept all",
                 [focused(role="push button", name_contains="Inspect data sent"),
                  focus_sequence(), speech_record()],
                 should="Tab goes on from where the reader was, to the Inspect accordion"):
        run.key("Tab")
    scene(run, "focus Reject all", lambda: run.grab_focus(role="push button", name="Reject all"))
    with run.act("Space on Reject all",
                 [focus_ends_on("push button", "Reject all"), no_focus_on_frame(),
                  focus_sequence(), speech_record(),
                  node_states("toggle button", SWITCH, absent="pressed")],
                 should="consent is refused: the switch shows off and cannot be used; focus "
                        "stays on Reject all",
                 tree=True, record=3.0):
        run.key("space")
    with run.act("Shift+Tab after Reject all",
                 [focus_sequence(), speech_record()],
                 should="Shift+Tab goes back from where the reader was (the switch, which is "
                        "now disabled, or the last intent button)",
                 tree=True):
        run.key("Shift+Tab")


def telemetry_withdraw(run):
    run.wait_for(role="push button", name="Accept all", timeout=30.0)
    scene(run, "grant consent through AT-SPI",
          lambda: run.action("click", role="push button", name="Accept all"), record=2.0)
    scene(run, "focus Withdraw consent",
          lambda: run.grab_focus(role="push button", name="Withdraw consent"))
    with run.act("Space on Withdraw consent opens the confirmation",
                 [announced("Withdraw"), focus_sequence(), speech_record(),
                  in_tree(role="alert")],
                 should="a question box opens, its title and text are spoken, and focus is "
                        "on its default button",
                 tree=True, record=3.0):
        run.key("space")
    with run.act("Enter confirms",
                 [focused(role="push button", name="Withdraw consent"),
                  said("Withdraw consent push button"), no_focus_on_frame(), focus_sequence(),
                  speech_record(), not_in_tree(role="alert"),
                  node_states("toggle button", SWITCH, absent="pressed")],
                 should="consent is withdrawn and focus goes back to Withdraw consent, which "
                        "the reader hears",
                 tree=True, record=3.0):
        run.key("Return")


def telemetry_inspect(run):
    intents = Printed(run, "intent dispatched: app.demo.click")
    run.wait_for(role="push button", name="Accept all", timeout=30.0)
    scene(run, "grant consent through AT-SPI",
          lambda: run.action("click", role="push button", name="Accept all"), record=2.0)
    scene(run, "focus Fire 'click' intent",
          lambda: run.grab_focus(role="push button", name="Fire 'click' intent"))
    with intents.act("Space on Fire 'click' intent",
                     [focus_stays(role="push button", name="Fire 'click' intent"),
                      in_tree(role="push button", name_contains="Inspect data sent (1"),
                      speech_record()],
                     should="the intent is sent and recorded; focus stays on the button",
                     tree=True, record=3.0):
        run.key("space")
    scene(run, "focus the Inspect accordion",
          lambda: run.grab_focus(role="push button", name_contains="Inspect data sent"))
    with run.act("Space expands the Inspect accordion",
                 [focus_stays(role="push button", name_contains="Inspect data sent"),
                  speech_record(), focus_sequence()],
                 should="the accordion opens, focus stays on its header; the recorded event "
                        "can be read below it",
                 tree=True, record=3.0):
        run.key("space")
    with intents.act("AT-SPI click on Fire 'click' intent while focus is on the accordion",
                     [focus_stays(role="push button", name_contains="Inspect data sent"),
                      no_focus_on_frame(), focus_sequence(), speech_record(),
                      in_tree(role="label", name_contains="2. intent.dispatched")],
                     should="a second event is recorded; focus stays where the reader is "
                            "(the accordion header)",
                     tree=True, record=3.0):
        run.action("click", role="push button", name="Fire 'click' intent")


# ---------------------------------------------------------------------------
# web-view-demo
# ---------------------------------------------------------------------------


def web_view(run):
    run.wait_for(role="push button", name="Native UI", timeout=30.0)
    with run.act("the window as launched",
                 [node_states("panel", name_contains="Loading", want="focusable"),
                  no_symbol_names(), not_in_tree(role="label", name=""),
                  web_view_tells_state(), speech_record()],
                 should="each control is named for what it does, and the web view says what "
                        "it holds",
                 tree=True, record=2.0):
        pass
    scene(run, "focus Native UI", lambda: run.grab_focus(role="push button", name="Native UI"))
    with run.act("Space on Native UI",
                 [focus_stays(role="push button", name="Native UI"), speech_record(),
                  said("Native UI tab"), in_tree(role="label", name_contains="Native UI tab"),
                  in_tree(role="label", name="This tab is pure Teksilo.")],
                 should="the native panel replaces the web view, and the reader is told what "
                        "is now shown (a sighted user sees the body change)",
                 tree=True, record=3.0):
        run.key("space")
    scene(run, "focus Browser", lambda: run.grab_focus(role="push button", name="Browser"))
    with run.act("Space on Browser",
                 [focus_stays(role="push button", name="Browser"), speech_record(),
                  said("Browser tab"), in_tree(role="label", name_contains="Browser tab")],
                 should="the web view comes back, and the reader is told", tree=True,
                 record=3.0):
        run.key("space")
    # Focus is only asked for on "Load example.com", never an activation: the
    # button would load a page from the network on an engine that works.
    scene(run, "focus the last toolbar button",
          lambda: run.grab_focus(role="push button", name="Load example.com"))
    with run.act("Tab to the web view",
                 [focused(role="panel"), said("Enter"), web_view_tells_state(), speech_record()],
                 should="focus reaches the web view, and the reader hears that it is web "
                        "content, that Enter goes into the page, and whether its page loaded",
                 tree=True, record=2.0):
        run.key("Tab")
    with run.act("Enter on the web view",
                 [focus_sequence(), speech_record()],
                 should="Enter hands the keyboard to the page; with no engine, the reader is "
                        "told nothing is there, and focus is not lost", tree=True, record=3.0):
        # Enter only where it cannot reach "Load example.com".
        holder = run.find(focused=True) or {}
        if holder.get("role") != "panel":
            raise RuntimeError(f"focus is on {holder.get('role')} {holder.get('name')!r}, "
                               "not the web view: Enter not pressed")
        run.key("Return")


SCENARIOS = [
    Scenario("rest-simple-button", "simple-button", simple_button,
             "the one button: reading, Space, Enter, AT-SPI click, Tab wrap"),
    Scenario("rest-simple-button-inspector", "simple-button", simple_button_inspector,
             "the debug inspector opened and closed with F12"),
    Scenario("rest-bridge-smoke", "automation_bridge_smoke", bridge_smoke,
             "the smoke window kept up with --serve", args=["--serve"]),
    Scenario("rest-telemetry-switch", "telemetry-plausible", telemetry_switch,
             "the consent switch turned on and off", env=dict(LOCAL_ENDPOINT)),
    Scenario("rest-telemetry-accept-reject", "telemetry-plausible", telemetry_accept_reject,
             "Accept all and Reject all", env=dict(LOCAL_ENDPOINT)),
    Scenario("rest-telemetry-withdraw", "telemetry-plausible", telemetry_withdraw,
             "Withdraw consent and its confirmation", env=dict(LOCAL_ENDPOINT)),
    Scenario("rest-telemetry-inspect", "telemetry-plausible", telemetry_inspect,
             "an intent recorded, and the Inspect accordion", env=dict(LOCAL_ENDPOINT)),
    Scenario("rest-web-view", "web-view-demo", web_view,
             "the chrome around the web view, with no engine"),
    tab_walk_scenario("rest-telemetry-tabwalk", "telemetry-plausible", stops=16,
                      env=dict(LOCAL_ENDPOINT)),
]
