# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""The scene examples and the small demos around them: what a reader gets.

Packages: scene-showcase, scene-corkboard, scene-magnetism, scene-ink,
over-constraint, theme-styles, animations, animations-kit, async-demo,
touch-playground, teksilo-widgets-previewer.

Where the reader's share is produced:

* `crates/teksilo-scene/src/view/a11y_impl.rs`: the `SceneView` is
  `Role::Pane` (AT-SPI "panel"), named only by `.a11y_label(..)`; every
  lightweight item is a synthetic node (`Role::GraphicsObject`, AT-SPI
  "panel", named by `access_label`, no actions, no focus, no selected state);
  a magnet is a synthetic `Role::Button` child of its item, with no action;
  in magnet connect mode the view points `active_descendant` at the focused
  magnet.
* `crates/teksilo-scene/src/view/magnetism.rs`, `handle_connect_key`: `m`
  toggles connect mode, arrows rove, Enter picks a source then connects, Esc
  cancels. Nothing in it announces.
* `crates/teksilo-scene/src/scene_card.rs`: one named `Role::Group` per card,
  a tab stop; Enter edits, Esc leaves.
* `crates/teksilo-scene/src/minimap.rs`: `Role::Group` "Scene minimap" whose
  reading of the viewport is a *string value*; arrows announce through
  `ctx.announce` (the framework announcer, K2).
* `crates/teksilo-widgets/src/progress_bar.rs` / `spinner.rs`: live polite
  `ProgressIndicator`s, announced by name whenever they enter the filtered
  tree.
* `crates/teksilo-widgets/src/animations/collapse.rs`: publishes nothing of
  its own, so its collapsed child is walked as if shown.
* `crates/teksilo-preview-ui/src/{navigator,toolbar,inspector,knob_form}.rs`:
  the previewer's rows, pickers, variant radios and knob editors.

Messages that go through the framework announcer (`ctx.announce`, so the K2
fix covers them): the minimap's arrow/Home moves. The corkboard's "Add Act"
is a live group (`SceneModel::set_a11y_live`), not the announcer. The
animations-kit progress bar and spinners are live nodes, not the announcer.
async-demo and touch-playground announce nothing at all.
"""

from __future__ import annotations

from reader_lib.checks import (_focus_node, _is_focus, announced, custom, event, focused,
                               in_tree, no_event, not_announced, not_in_tree, not_said, said)
from reader_lib.orca import normalized, utterances
from reader_lib.run import RunError
from reader_lib.scenario import Scenario


# ---------------------------------------------------------------------------
# Checks of our own
# ---------------------------------------------------------------------------


def _n(text: str | None) -> str:
    return normalized(text or "")


def _heard(act) -> list[str]:
    return [f"{u.stamp} said{' (cut)' if u.cut else ''}: {u.text!r}" for u in utterances(act.orca)]


def _line(act, e) -> str:
    s = e.get("source", {})
    t = e.get("target")
    extra = f" -> [{t.get('role')}] {t.get('name')!r}" if t else ""
    txt = f" text={e.get('text')!r}" if e.get("text") else ""
    d = f" {e.get('detail1')}" if e["type"].startswith("object:state") else ""
    return f"{act.rel_ms(e):+8.1f} ms {e['type']}{d} [{s.get('role')}] {s.get('name')!r}{extra}{txt}"


def focus_named():
    """The act's last focus change lands on a node that has a name."""
    def run(act):
        moves = [e for e in act.events if _is_focus(e)]
        if not moves:
            return False, ["no focus change on the bus in this act"]
        node = _focus_node(moves[-1])
        return bool((node.get("name") or "").strip()), [_line(act, e) for e in moves]
    return custom("focus lands on a node with a name", run)


def said_something():
    def run(act):
        heard = _heard(act)
        return bool(heard), heard or ["Orca said nothing in this act"]
    return custom("Orca says something", run, needs_orca=True)


def said_any(*texts: str):
    def run(act):
        ok = any(_n(t) in _n(u.text) for u in utterances(act.orca) for t in texts)
        return ok, _heard(act) or ["Orca said nothing"]
    return custom(f"Orca says any of {texts!r}", run, needs_orca=True)


def utterances_short(limit: int):
    """No single utterance longer than `limit` characters."""
    def run(act):
        long = [u for u in utterances(act.orca) if len(u.text) > limit]
        return not long, [f"{len(u.text)} characters: {u.text[:160]!r}…" for u in long] or \
            _heard(act)
    return custom(f"no utterance longer than {limit} characters", run, needs_orca=True)


def announced_times(text: str, times: int):
    def run(act):
        found = [e for e in act.events if e["type"] == "object:announcement"
                 and _n(text) in _n(e.get("text"))]
        return len(found) == times, [f"{len(found)} announcement(s) of {text!r}"] + \
            [_line(act, e) for e in found]
    return custom(f"exactly {times} announcement(s) of {text!r}", run)


def said_times(text: str, times: int):
    def run(act):
        heard = [u for u in utterances(act.orca) if _n(text) in _n(u.text)]
        return len(heard) == times, [f"Orca said {text!r} {len(heard)} time(s)"] + _heard(act)
    return custom(f"Orca says {text!r} exactly {times} time(s)", run, needs_orca=True)


def quiet(at_most: int = 0, skip_bounds: bool = True):
    """The act (an idle window) carries at most `at_most` events."""
    def run(act):
        found = [e for e in act.events if not e["type"].startswith("harness:")
                 and not (skip_bounds and e["type"] == "object:bounds-changed")]
        return len(found) <= at_most, [f"{len(found)} event(s) in "
                                       f"{(act.end_mono - act.start_mono):.1f} s"] + \
            [_line(act, e) for e in found[:20]]
    return custom(f"at most {at_most} event(s) on the bus while idle", run)


def no_orca_defunct_drop():
    def run(act):
        drops = [f"{ln.stamp} {ln.text}" for ln in act.orca if ln.is_defunct_drop]
        return not drops, drops
    return custom("Orca drops no event as defunct", run, needs_orca=True)


def focus_is_text_target():
    """Text typed in the act changes the node that has focus, so a reader's
    echo and caret tracking follow it."""
    def run(act):
        focus_path = None
        for e in act.history:
            if _is_focus(e):
                focus_path = _focus_node(e).get("path")
        changes = [e for e in act.events if e["type"].startswith("object:text-changed")]
        if not changes:
            return False, ["no text change in the act"]
        ok = any(e["source"].get("path") == focus_path for e in changes)
        return ok, [f"focus path ...{(focus_path or '')[-14:]}"] + \
            [_line(act, e) + f" path ...{e['source'].get('path', '')[-14:]}" for e in changes]
    return custom("the text that changes is the node focus is on", run)


def outcome(holder: dict, key: str, describe: str):
    """A check on something the scenario body recorded while acting."""
    def run(act):
        value = holder.get(key)
        return bool(value and value[0]), [value[1] if value else "not recorded"]
    return custom(describe, run)


def tree_node(role: str, name: str, predicate, describe: str):
    def run(act):
        from reader_lib.checks import _walk
        for node in _walk(act.tree):
            if node.get("role") == role and (node.get("name") or "") == name:
                ok, why = predicate(node)
                return ok, [f"[{role}] {name!r}: {why}"]
        return False, [f"no [{role}] {name!r} in the tree"]
    return custom(describe, run, needs_tree=True)


def _try(holder: dict, key: str, fn) -> None:
    try:
        reply = fn()
        holder[key] = (True, f"done: {reply}")
    except RunError as exc:
        holder[key] = (False, f"refused: {exc}")


# ---------------------------------------------------------------------------
# scene-magnetism: the keyboard connect flow
# ---------------------------------------------------------------------------


def magnet_connect(run):
    run.wait_for(role="push button", name="Input output")
    with run.act("Tab into the scene", [focused(role="panel"), focus_named()],
                 should="focus lands on the node graph, and the reader hears what it is"):
        run.key("Tab")
    with run.act("m enters connect mode",
                 [focused(role="push button", name="Input output"), said("Input output"),
                  said_any("connect")],
                 should="the reader lands on the first port and is told that connect mode is on"):
        run.key("m")
    with run.act("Enter picks Input output as the source",
                 [said_any("source", "selected", "pressed", "picked")],
                 should="the reader is told the port is now the pending source"):
        run.key("Enter")
    with run.act("Right arrow to a compatible input",
                 [focused(role="push button"), focus_named(), said_any("input")],
                 should="focus roves to a compatible port and the reader hears its name"):
        run.key("Right")
    landed = run.last_focus().get("name") or "?"
    run.note(f"Right arrow landed on {landed!r}")
    with run.act("Enter connects the two ports",
                 [said_any("connect"), in_tree(role="panel", name="connection"),
                  tree_node("panel", "Input", lambda n: (
                      "flows-to" in (n.get("relations") or {}),
                      f"relations={n.get('relations')}"),
                      "the source node carries a flows-to relation")],
                 should="the connection is made and the reader is told so"):
        run.key("Enter")
    with run.act("Escape leaves connect mode", [focused(role="panel"), said_something()],
                 should="focus returns to the graph and the reader hears where they are"):
        run.key("Escape")
    holder: dict = {}
    with run.act("AT-SPI click on a port (a reader's own activation)",
                 [outcome(holder, "click", "a port offers an activation a reader can use")],
                 should="a port that is exposed as a push button can be pressed"):
        _try(holder, "click", lambda: run.action("click", role="push button",
                                                 name="Blur output"))


# ---------------------------------------------------------------------------
# scene-showcase: the scene pane, lightweight items, the minimap, the cards
# ---------------------------------------------------------------------------


def showcase(run):
    run.wait_for(role="panel", name="Scene minimap")
    with run.act("(scene) Tab through the toolbar", should="scene setting, not judged"):
        run.key("Tab", "Tab", "Tab", gap=0.6)
    with run.act("Tab onto the scene", [focused(role="panel"), focus_named(),
                                        utterances_short(300)],
                 should="focus lands on the scene and the reader hears its name, not the "
                        "text of everything in it"):
        run.key("Tab")
    with run.act("Right arrow pans the scene", [said_any("pan", "moved", "viewport")],
                 should="the reader is told the view moved"):
        run.key("Right")
    holder: dict = {}
    with run.act("AT-SPI click on a lightweight item",
                 [outcome(holder, "click", "a lightweight item can be activated")],
                 should="a reader can select an item such as 'draggable 1'"):
        _try(holder, "click", lambda: run.action("click", role="panel", name="draggable 1"))
    with run.act("AT-SPI grab_focus on a lightweight item",
                 [focused(role="panel", name="draggable 1")],
                 should="a reader can move focus to an item such as 'draggable 1'"):
        run.grab_focus(role="panel", name="draggable 1")
    with run.act("(scene) focus the minimap", should="scene setting, not judged"):
        run.grab_focus(role="panel", name="Scene minimap")
    with run.act("(scene) Shift+Tab twice", should="scene setting, not judged"):
        run.key("Shift+Tab", "Shift+Tab", gap=0.8)
    with run.act("Shift+Tab onto the card button",
                 [focused(role="push button", name="Click me"), said("Click me")],
                 should="the card's button is reached"):
        run.key("Shift+Tab")
    with run.act("Tab to the card combo box", [focused(role="combo box"), focus_named()],
                 should="the combo box has a name"):
        run.key("Tab")
    with run.act("Tab to the nested scene", [focused(role="landmark", name="Inner scene")],
                 should="the nested scene is reached by name"):
        run.key("Tab")
    with run.act("Tab onto the minimap", [focused(role="panel", name="Scene minimap"),
                                          said_any("viewport")],
                 should="the reader hears where the viewport is, which is all the minimap "
                        "shows"):
        run.key("Tab")
    for i in (1, 2, 3):
        with run.act(f"Right arrow on the minimap, press {i}",
                     [announced("Viewport"), said_any("viewport")],
                     should="the view moves and the reader hears where it now is"):
            run.key("Right")


# ---------------------------------------------------------------------------
# scene-corkboard
# ---------------------------------------------------------------------------


def corkboard_add_act(run):
    run.wait_for(role="push button", name="Add Act")
    run.grab_focus(role="push button", name="Add Act")
    with run.act("Space on Add Act", [announced("Act 4"), said("Act 4"),
                                      announced_times("Act 4", 1), said_times("Act 4", 1)],
                 should="the new act is announced once"):
        run.key("space")
    with run.act("Space on Add Act again", [announced("Act 5"), said_times("Act 5", 1)],
                 should="the next new act is announced once"):
        run.key("space")


def corkboard_cards(run):
    run.wait_for(role="push button", name="Add Act")
    with run.act("(scene) focus the Theme combo box", should="scene setting, not judged"):
        run.grab_focus(role="combo box", name="Theme")
    with run.act("Tab onto the main pane", [focused(role="panel"), focus_named()],
                 should="focus lands on the corkboard and the reader hears its name"):
        run.key("Tab")
    with run.act("Tab onto the first card",
                 [focused(role="panel", name="Act I — Opening"), said("Act I — Opening")],
                 should="the reader hears the card's act and its title"):
        run.key("Tab")
    with run.act("Tab into the card's prose",
                 [focused(role="entry"), focus_named()],
                 should="focus lands on the note's text, and the reader hears an editable "
                        "text with a name"):
        run.key("Tab")
    with run.act("type a word", [focus_is_text_target()],
                 should="the characters land in the node the reader is on"):
        run.type("zz")
    with run.act("Tab in the prose", [focused(role="panel", name="Inciting Incident")],
                 should="Tab moves on to the next card"):
        run.key("Tab")
    with run.act("Escape", [focused(role="panel", name="Act I — Opening"), said_something()],
                 should="Esc leaves the text for the card, and the reader hears it"):
        run.key("Escape")
    with run.act("Ctrl+Tab", [focused(role="panel", name="Inciting Incident")],
                 should="the next card is reached"):
        run.key("Ctrl+Tab")
    with run.act("Shift+Tab back to the first card's prose",
                 [said_something(), no_orca_defunct_drop()],
                 should="the reader hears where focus went"):
        run.key("Shift+Tab")
    with run.act("Shift+Tab to the first card", [focused(role="panel", name="Act I — Opening"),
                                                 said("Act I — Opening")],
                 should="the card is read"):
        run.key("Shift+Tab")
    with run.act("Enter on the card (edit)", [said_something(), no_orca_defunct_drop()],
                 should="the card enters editing and the reader hears the text they are in"):
        run.key("Enter")
    with run.act("Escape out of editing", [focused(role="panel", name="Act I — Opening"),
                                           said("Act I — Opening")],
                 should="the card is read again"):
        run.key("Escape")


# ---------------------------------------------------------------------------
# scene-ink
# ---------------------------------------------------------------------------


def ink(run):
    run.wait_for(role="panel")
    for i in range(1, 5):
        with run.act(f"Tab {i}", [focus_named(), said_something(), no_orca_defunct_drop()],
                     should="focus moves and the reader hears where it went"):
            run.key("Tab")
    with run.act("Backspace (remove the last stroke; there is none)",
                 [said_something()],
                 should="the reader is told what Backspace did, even if nothing"):
        run.key("BackSpace")


# ---------------------------------------------------------------------------
# animations / animations-kit
# ---------------------------------------------------------------------------


def animations_idle(run):
    run.wait_for(role="page tab", name="Animated")
    with run.act("idle on the Animated tab", [quiet(0)], record=6.0,
                 should="three looping progress bars put nothing on the bus"):
        pass
    run.grab_focus(role="page tab", name="Animated")
    with run.act("Right arrow to the Static tab", [said("Static")],
                 should="the Static tab is selected and read"):
        run.key("Right")
    with run.act("idle on the Static tab", [quiet(0)], record=6.0,
                 should="nothing on the bus"):
        pass


def animkit(run):
    run.wait_for(role="toggle button", name="Animate me")
    with run.act("idle at the top (spinners, sweep)", [quiet(0)], record=6.0,
                 should="looping spinners put nothing on the bus"):
        pass
    with run.act("Collapse is collapsed at launch",
                 [not_in_tree(name="Hidden content #1")], tree=True,
                 should="a collapsed section is not read as present"):
        pass
    with run.act("(scene) focus Toggle Fade", should="scene setting, not judged"):
        run.grab_focus(role="push button", name="Toggle Fade")
    with run.act("Space on Toggle Fade (fades the content out)",
                 [not_in_tree(name_contains="Faded content")], tree=True, record=3.0,
                 should="content faded to nothing is not read as present"):
        run.key("space")
    with run.act("Tab from Toggle Fade to the next control (scrolls the Loading bar in)",
                 [not_announced("Loading"), not_said("Loading"),
                  focused(role="push button", name="Hover or hold me")],
                 should="a progress bar that was always there is not announced because the "
                        "view scrolled to it"):
        run.key("Tab")
    with run.act("(scene) Tab on four times (the bar scrolls out)",
                 should="scene setting, not judged"):
        run.key("Tab", "Tab", "Tab", "Tab", gap=0.8)
    with run.act("Shift+Tab back to Hover or hold me (the bar scrolls back in)",
                 [not_announced("Loading"), not_said("Loading")],
                 should="no announcement from scrolling"):
        run.key("Shift+Tab", "Shift+Tab", "Shift+Tab", "Shift+Tab", gap=0.8)
    with run.act("(scene) focus Toggle content", should="scene setting, not judged"):
        run.grab_focus(role="push button", name="Toggle content")
    with run.act("idle beside the Cycle", [quiet(0)], record=7.0,
                 should="how much a rotating Cycle puts on the bus while idle"):
        pass


# ---------------------------------------------------------------------------
# async-demo
# ---------------------------------------------------------------------------


def async_demo(run):
    run.wait_for(role="push button", name="Load data (spawn_blocking)")
    run.grab_focus(role="push button", name="Load data (spawn_blocking)")
    with run.act("Space on Load data, wait for the worker",
                 [said_any("Loading"), said_any("Done")], record=4.0,
                 should="the reader hears that loading started and that it finished"):
        run.key("space")
    run.grab_focus(role="push button", name="Fetch + open result window (spawn_local_with)")
    with run.act("Space on Fetch + open result window",
                 [said_any("Async result"), said_any("Delivered")], record=5.0,
                 should="the new window is announced with its title and its content read"):
        run.key("space")


# ---------------------------------------------------------------------------
# touch-playground: the density switch
# ---------------------------------------------------------------------------


def touch_density(run):
    run.wait_for(role="radio button", name="Compact")
    with run.act("Tab onto the density control",
                 [focused(role="radio button", name="Compact"), not_said("not selected")],
                 should="the reader hears Compact, and that it is the current density"):
        run.key("Tab")
    with run.act("Right arrow: switch to Comfortable",
                 [said_any("density", "Comfortable"), focus_named(),
                  focused(role="radio button", name="Comfortable")],
                 should="the density changes, the reader is told so (the page says 'a screen "
                        "reader is told once'), and focus stays on the control"):
        run.key("Right")
    with run.act("Tab onward after the switch", [focus_named(), said_something()],
                 should="Tab continues from the density control"):
        run.key("Tab")


# ---------------------------------------------------------------------------
# over-constraint: the toolbar
# ---------------------------------------------------------------------------


def overconstraint(run):
    run.wait_for(role="tool bar")
    with run.act("Tab into the toolbar", [focused(role="combo box"), focus_named()],
                 should="the view-mode combo box has a name"):
        run.key("Tab")
    with run.act("Right arrow to Confirm", [focused(role="push button", name="Confirm"),
                                            said_times("Confirm", 1)],
                 should="the icon button is read once, by its name"):
        run.key("Right")
    with run.act("Right arrow to New Document",
                 [focused(role="push button", name="New Document"),
                  said_times("New Document", 1)],
                 should="read once"):
        run.key("Right")


# ---------------------------------------------------------------------------
# theme-styles
# ---------------------------------------------------------------------------


def theme_styles(run):
    run.wait_for(role="toggle button", name="Notifications")
    with run.act("(scene) focus the Notifications toggle", should="scene setting, not judged"):
        run.grab_focus(role="toggle button", name="Notifications")
    with run.act("Space on the Notifications toggle",
                 [event("object:state-changed:pressed", role="toggle button",
                        name_contains="Notifications"), said("pressed")], record=4.0,
                 should="the toggle turns on and the reader hears it"):
        run.key("space")
    with run.act("Tab to the next toggle", [focused(role="toggle button", name="Dark mode"),
                                            no_event("object:state-changed:pressed",
                                                     name_contains="Notifications")],
                 should="nothing about the first toggle is still pending"):
        run.key("Tab")
    with run.act("(scene) focus BRUTAL", should="scene setting, not judged"):
        run.grab_focus(role="push button", name="BRUTAL")
    with run.act("Space on the per-call styled BRUTAL button",
                 [no_event("object:state-changed:defunct")],
                 should="a custom Tier-3 style keeps the button a button"):
        run.key("space")


# ---------------------------------------------------------------------------
# teksilo-widgets-previewer
# ---------------------------------------------------------------------------


def previewer_nav(run):
    run.wait_for(role="push button", name="fr-FR")
    with run.act("(scene) focus fr-FR", should="scene setting, not judged"):
        run.grab_focus(role="push button", name="fr-FR")
    with run.act("Tab into the navigator", [focus_named(), said_any("TextScaleControl")],
                 should="the reader hears the widget the row is for"):
        run.key("Tab")
    with run.act("Enter on the navigator row",
                 [in_tree(name_contains="text_scale_control")], tree=True,
                 should="the row's widget opens in the canvas"):
        run.key("Enter")
    with run.act("Space on the navigator row",
                 [in_tree(name_contains="text_scale_control")], tree=True,
                 should="the row's widget opens in the canvas"):
        run.key("space")
    holder: dict = {}
    with run.act("AT-SPI click on the focused navigator row",
                 [outcome(holder, "click", "the row offers an activation"),
                  in_tree(name_contains="text_scale_control")], tree=True,
                 should="a reader's own activation opens the row's widget"):
        _try(holder, "click", lambda: run.action("click", focused=True))
    for i in (2, 3):
        with run.act(f"Tab to navigator row {i}", [focus_named(), said_something()],
                     should="the reader hears the row"):
            run.key("Tab")
    with run.act("(scene) focus Save PNG", should="scene setting, not judged"):
        run.grab_focus(role="push button", name="Save PNG…")
    with run.act("Tab onto the theme picker's Native button",
                 [focused(role="push button", name="Native"),
                  said_any("pressed", "selected", "current")],
                 should="the reader can tell which theme is active"):
        run.key("Tab")


def previewer_knobs(run):
    """The Label knob's entry has focus at launch; Orca reads it then."""
    run.wait_for(role="toggle button", name="Enabled")
    with run.act("(scene) focus the Label knob", should="scene setting, not judged"):
        run.grab_focus(role="entry")
    with run.act("Tab to the Variant knob", [focused(role="combo box"), focus_named()],
                 should="named 'Variant'"):
        run.key("Tab")
    with run.act("Tab to the Enabled knob", [focused(role="toggle button", name="Enabled")],
                 should="named 'Enabled'"):
        run.key("Tab")
    with run.act("Shift+Tab twice back to the Label knob",
                 [focused(role="entry"), focus_named()],
                 should="the Label knob's editor is named by its row label"):
        run.key("Shift+Tab", "Shift+Tab", gap=0.8)
    with run.act("(scene) focus the first variant radio", should="scene setting, not judged"):
        run.grab_focus(role="radio button", name="")
    with run.act("Tab and Shift+Tab back to the variant radio", [focus_named()],
                 should="a variant radio is named by its variant"):
        run.key("Tab", "Shift+Tab", gap=0.8)


# ---------------------------------------------------------------------------
# scene-corkboard: the overview pane in the Tab ring
# ---------------------------------------------------------------------------


def corkboard_overview(run):
    run.wait_for(role="landmark", name="Overview pane")
    with run.act("(scene) focus the main pane's last card", should="scene setting, not judged"):
        run.grab_focus(role="panel", name="Coda")
    stops = []
    for i in range(1, 5):
        with run.act(f"Ctrl+Tab {i} from the last card", should="where the Tab ring goes "
                     "after the main pane's last card") as act:
            run.key("Ctrl+Tab")
        run.collect_events(act)
        node = run.last_focus()
        stops.append(f"{node.get('role')} {node.get('name')!r}")
    run.note("Ctrl+Tab stops after the main pane's 'Coda': " + "; ".join(stops))


# ---------------------------------------------------------------------------
# over-constraint: narrow the window, so the toolbar overflows
# ---------------------------------------------------------------------------


def _resize(run, width: int, height: int) -> None:
    """Resize this application's window through the private KWin, as a user
    dragging its edge would. Uses the run's own KWin script loader."""
    assert run.app is not None
    run._step(f"resize the window to {width}x{height} through KWin")
    script = run.work / f"resize-{run.app.pid}-{width}.js"
    script.write_text(
        "const wins = workspace.windowList ? workspace.windowList() : workspace.clientList();\n"
        f"wins.forEach(w => {{ if (w.pid === {run.app.pid}) {{ const g = w.frameGeometry; "
        f"w.frameGeometry = {{x: g.x, y: g.y, width: {width}, height: {height}}}; }} }});\n",
        encoding="utf-8")
    run._kwin_script(script)


def toolbar_fits():
    """After narrowing, every toolbar control lies inside the window: the bar
    collapsed what does not fit into its overflow button."""
    def run_check(act):
        from reader_lib.checks import _walk
        frame = next((n for n in _walk(act.tree) if n.get("role") == "frame"), None)
        width = (frame or {}).get("extents", [0, 0, 0, 0])[2]
        out = [f"[{n.get('role')}] {n.get('name')!r} at {n.get('extents')}"
               for n in _walk(act.tree)
               if n.get("role") in ("push button", "combo box", "tool bar")
               and n.get("extents") and (n["extents"][0] < 0 or n["extents"][0] > width)]
        return not out, [f"window width {width}"] + out
    return custom("every toolbar control lies inside the narrowed window", run_check,
                  needs_tree=True)


def overconstraint_narrow(run):
    run.wait_for(role="tool bar")
    with run.act("narrow the window to 420 px", [toolbar_fits()], tree=True,
                 should="the toolbar collapses its actions into an overflow button"):
        _resize(run, 420, 640)
        run.wait(1.5)
    with run.act("(scene) Tab into the toolbar", should="scene setting, not judged"):
        run.key("Tab")
    for i in range(1, 5):
        with run.act(f"Right arrow {i} along the narrowed toolbar",
                     [focus_named(), said_something()],
                     should="each remaining control, then the overflow button, is read by "
                            "name"):
            run.key("Right")


# ---------------------------------------------------------------------------
# scene-corkboard: the selection transform controller from the keyboard
# ---------------------------------------------------------------------------


def _in_overview(run, act) -> bool | None:
    """Whether the act's last focus change landed inside the Overview pane."""
    from reader_lib.audit import focus_path
    run.collect_events(act)
    moves = [e for e in act.events if _is_focus(e)]
    if not moves:
        return None
    path = _focus_node(moves[-1]).get("path")
    tree = run.tree_now()
    chain = focus_path(tree, path) if path else []
    return any(n.get("name") == "Overview pane" for n in chain)


def not_in_overview(holder: dict):
    def run_check(act):
        where = holder.get("where")
        return where is False, [f"focus in the Overview pane: {where}"]
    return custom("focus stays in the pane the user acted in (not the Overview pane)",
                  run_check)


def corkboard_transform(run):
    run.wait_for(role="push button", name="Add Act")
    with run.act("(scene) focus the first card", should="scene setting, not judged"):
        run.grab_focus(role="panel", name="Act I — Opening")
    with run.act("the focused card is selected",
                 [in_tree(role="panel", name="Act I — Opening", state="selected")], tree=True,
                 should="SceneCard: the Focus action selects the card"):
        pass
    holder: dict = {}
    with run.act("Enter on the main pane's card (edit)", [not_in_overview(holder)],
                 should="focus goes into this card's prose, in the main pane") as act:
        run.key("Enter")
    holder["where"] = _in_overview(run, act)
    with run.act("Escape (back to the card, selected)", [focused(role="panel",
                                                                 name="Act I — Opening")],
                 should="focus returns to the card"):
        run.key("Escape")
    with run.act("Shift+Tab to the pane", [focused()],
                 should="focus returns to the pane the card is in"):
        run.key("Shift+Tab")
    with run.act("t enters transform mode", [focus_named(), said_something()],
                 should="the reader lands on a named handle of the selection frame"):
        run.key("t")
    landed = run.last_focus()
    run.note(f"t landed on [{landed.get('role')}] {landed.get('name')!r}")
    with run.act("Right arrow steps the handle", [said_something()],
                 should="the reader hears what changed"):
        run.key("Right")
    with run.act("Enter commits", [said_something()],
                 should="the reader hears that the change was applied"):
        run.key("Enter")
    with run.act("Escape leaves transform mode", [focused(role="panel"), said_something()],
                 should="focus returns to the pane"):
        run.key("Escape")


SCENARIOS = [
    Scenario("sceneetc-magnet-connect", "scene-magnetism", magnet_connect,
             "m, arrows, Enter, Enter: the keyboard connect flow, and a port's activation"),
    Scenario("sceneetc-showcase", "scene-showcase", showcase,
             "the scene pane, a lightweight item, a card's controls, the minimap"),
    Scenario("sceneetc-corkboard-add-act", "scene-corkboard", corkboard_add_act,
             "Add Act twice: a live Act group announced"),
    Scenario("sceneetc-corkboard-cards", "scene-corkboard", corkboard_cards,
             "Tab through a card into its prose and out again"),
    Scenario("sceneetc-ink", "scene-ink", ink,
             "Tab round the ink page: the focusable page and its notes"),
    Scenario("sceneetc-animations-idle", "animations", animations_idle,
             "idle windows on the looping progress bars and on the static tab"),
    Scenario("sceneetc-animkit", "animations-kit", animkit,
             "idle spinners, a scrolled-in live progress bar, Cycle, Collapse, Fade"),
    Scenario("sceneetc-async", "async-demo", async_demo,
             "async completions: a status line and a result window"),
    Scenario("sceneetc-touch-density", "touch-playground", touch_density,
             "the density control and a density switch"),
    Scenario("sceneetc-overconstraint", "over-constraint", overconstraint,
             "the overflowing toolbar's controls"),
    Scenario("sceneetc-overconstraint-narrow", "over-constraint", overconstraint_narrow,
             "narrow the window so the toolbar overflows, then walk it"),
    Scenario("sceneetc-corkboard-transform", "scene-corkboard", corkboard_transform,
             "the selection frame's handles from the keyboard (t, Tab, arrows, Enter)"),
    Scenario("sceneetc-corkboard-overview", "scene-corkboard", corkboard_overview,
             "where the Tab ring goes after the main pane's last card"),
    Scenario("sceneetc-theme-styles", "theme-styles", theme_styles,
             "custom Tier-3 styles keep their controls' semantics"),
    Scenario("sceneetc-previewer-nav", "teksilo-widgets-previewer", previewer_nav,
             "the previewer's navigator rows and toolbar pickers"),
    Scenario("sceneetc-previewer-knobs", "teksilo-widgets-previewer", previewer_knobs,
             "the knob form for Button", args=["--widget=button"]),
]
