# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""spin-box: every SpinBox of the gallery, as a screen reader meets it.

Where each piece of a reader's experience is produced
(`crates/teksilo-widgets/src/spin_box.rs`):

* The editing field is the spin button: the `access_customize` in `build`
  gives the `TextInputField` `Role::SpinButton`, the label as its name, the
  numeric value / min / max / step / jump, and `Increment` / `Decrement`.
  The field's own `accessibility`
  (`primitives/text_input_field/widget_impl.rs`) adds the text as its value,
  the text runs and the selection.
* The composite and the step buttons are `GenericContainer`s, so they collapse
  (`accessibility` in `spin_box.rs`, `spin_box/step_button.rs`).
* The suffix (" pt", " dB", ...) is painted beside the text and is in neither
  the text nor the value.
* `special_value_text` ("Auto" on Timeout) is shown at the minimum, focused
  or not (`format_for_display`); keyboard focus selects it.
* The chrome (`styles/recipe_spin_box_style.rs`) puts a `Divider` between
  the field and the buttons, and `Divider::accessibility` claims
  `Role::Splitter`.

Every act is recorded, the scene setting included, so no event reaches the
bus between acts: the harness matches Orca's receipts to an act's events by
type, and an unrecorded focus change between two acts is matched as the next
act's (see harness_issues in the report).
"""

from __future__ import annotations

import time

from reader_lib.checks import (_focus_node, _is_focus, custom, event, focused, in_tree,
                               no_event, not_said, said)
from reader_lib.orca import normalized, utterances
from reader_lib.run import RunError
from reader_lib.scenario import Scenario


# ---------------------------------------------------------------------------
# Checks of our own
# ---------------------------------------------------------------------------


def _walk(tree):
    if not tree:
        return
    stack = [tree]
    while stack:
        node = stack.pop()
        yield node
        stack.extend(reversed(node.get("children", [])))


def said_exactly(text: str):
    """Some whole utterance of the act is `text` (case, spaces and a final
    period aside): the bare number Orca speaks for a spin button's value."""
    def run(act):
        heard = utterances(act.orca)
        ok = any(normalized(u.text).rstrip(".") == normalized(text).rstrip(".") and not u.cut
                 for u in heard)
        return ok, [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}" for u in heard] \
            or ["Orca said nothing in the act"]
    return custom(f"Orca says exactly {text!r}", run, needs_orca=True)


def said_something():
    def run(act):
        heard = utterances(act.orca)
        return bool(heard), [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                             for u in heard] or ["Orca said nothing in the act"]
    return custom("Orca says something", run, needs_orca=True)


def spin_value(name: str, current: float | None = None, text: str | None = None):
    """After the act, the spin button `name` exposes `current` on its Value
    interface and `text` on its Text interface."""
    def run(act):
        for node in _walk(act.tree):
            if node.get("role") == "spin button" and node.get("name") == name:
                value = node.get("value", {})
                shown = node.get("text", {}).get("text")
                ok = (current is None or abs((value.get("current") or 0.0) - current) < 1e-9) \
                    and (text is None or shown == text)
                return ok, [f"[spin button] {name!r} value={value} text={shown!r} "
                            f"states={node.get('states')}"]
        return False, [f"no spin button named {name!r} in the tree after the act"]
    want = []
    if current is not None:
        want.append(f"value {current:g}")
    if text is not None:
        want.append(f"text {text!r}")
    return custom(f"[spin button] {name!r} has {' and '.join(want)}", run, needs_tree=True)


def caret_at(name: str, caret: int, text: str | None = None):
    """After the act, the spin button's caret (Text.caretOffset) is `caret`."""
    def run(act):
        for node in _walk(act.tree):
            if node.get("role") == "spin button" and node.get("name") == name:
                t = node.get("text", {})
                ok = t.get("caret") == caret and (text is None or t.get("text") == text)
                return ok, [f"[spin button] {name!r} text={t}"]
        return False, [f"no spin button named {name!r}"]
    return custom(f"[spin button] {name!r} caret at {caret}"
                  + (f", text {text!r}" if text is not None else ""), run, needs_tree=True)


def spin_state(name: str, state: str, present: bool = True):
    def run(act):
        for node in _walk(act.tree):
            if node.get("role") == "spin button" and node.get("name") == name:
                has = state in node.get("states", [])
                return has == present, [f"[spin button] {name!r} states={node.get('states')}"]
        return False, [f"no spin button named {name!r}"]
    return custom(f"[spin button] {name!r} {'has' if present else 'lacks'} state {state!r}",
                  run, needs_tree=True)


def no_role(role: str):
    def run(act):
        found = [n for n in _walk(act.tree) if n.get("role") == role]
        return not found, [f"[{n.get('role')}] {n.get('name')!r} extents={n.get('extents')} "
                           f"index_in_parent={n.get('index_in_parent')}" for n in found] \
            or [f"no [{role}] in the tree"]
    return custom(f"the tree holds no [{role}]", run, needs_tree=True)


def units_reach_reader():
    """Each spin button with a suffix carries its unit, as a word, somewhere a
    reader can get it: its name, description, text or Value text."""
    units = {"Font size": "pt", "Gain": "dB", "Opacity": "%", "Timeout": "s",
             "Frequency": "Hz", "Font size, no buttons": "pt", "Font size mirror": "pt"}

    def run(act):
        ok, evidence = True, []
        for node in _walk(act.tree):
            name = node.get("name")
            if node.get("role") != "spin button" or name not in units:
                continue
            fields = [node.get("description"), (node.get("text") or {}).get("text"),
                      (node.get("value") or {}).get("text"), name]
            unit = units[name]
            found = any(unit in str(f or "").replace("\u202f", " ").split() for f in fields)
            ok = ok and found
            evidence.append(f"{name!r} (unit {unit!r}): description={node.get('description')!r} "
                            f"text={(node.get('text') or {}).get('text')!r} "
                            f"Value.text={(node.get('value') or {}).get('text')!r} "
                            f"attributes={node.get('attributes')}")
        return ok, evidence
    return custom("every spin button with a suffix exposes its unit", run, needs_tree=True)


# ---------------------------------------------------------------------------
# Scene setting, always inside an act
# ---------------------------------------------------------------------------


def setup_focus(run, name: str, *, role: str = "spin button") -> None:
    """Put focus on a control through AT-SPI, inside an act of its own."""
    run.wait_for(role=role, name=name)
    with run.act(f"setup: focus {name!r} (AT-SPI grab_focus)",
                 [focused(role=role, name=name)],
                 should="scene setting: focus is on the control"):
        run.grab_focus(role=role, name=name)


def tab_until(run, role: str, name: str, *, chord: str = "Tab", limit: int = 24) -> None:
    """Press `chord` until the bus reports focus on [role] name, inside one act."""
    with run.act(f"setup: {chord} until {name!r}", [focused(role=role, name=name)],
                 should="scene setting: focus is on the control"):
        for _ in range(limit):
            node = run.last_focus()
            if node.get("role") == role and node.get("name") == name:
                return
            run.key(chord)
            time.sleep(0.5)
        node = run.last_focus()
        if not (node.get("role") == role and node.get("name") == name):
            raise RunError(f"{limit} x {chord} never focused [{role}] {name!r}; "
                           f"last focus {node}")


# ---------------------------------------------------------------------------
# The tree at launch
# ---------------------------------------------------------------------------


def tree_body(run):
    run.wait_for(role="spin button", name="Font size")
    with run.act("the tree after launch",
                 [no_role("separator"), units_reach_reader(),
                  spin_state("Font size mirror", "read-only"),
                  spin_state("Font size mirror", "sensitive"),
                  spin_state("Font size mirror", "enabled"),
                  in_tree(role="spin button", name="Population"),
                  in_tree(role="spin button", name="Port"),
                  in_tree(role="push button", name="Reset all")],
                 should="every spin button, named, with its value and unit; nothing a reader "
                        "must step over that is not content; the whole page reachable",
                 record=1.0, tree=True):
        pass
    tree = run.acts[-1].tree
    for node in _walk(tree):
        if node.get("role") == "spin button":
            run.note(f"[spin button] {node.get('name')!r} states={node.get('states')} "
                     f"value={node.get('value')} text={node.get('text')} "
                     f"interfaces={node.get('interfaces')} actions={node.get('actions')} "
                     f"attributes={node.get('attributes')} "
                     f"description={node.get('description')!r} children={node.get('child_count')}")


# ---------------------------------------------------------------------------
# Stepping with the keys
# ---------------------------------------------------------------------------


def steps_body(run):
    run.wait_for(role="spin button", name="Font size")
    with run.act("focus Font size (AT-SPI grab_focus)",
                 [focused(role="spin button", name="Font size"), said("Font size"),
                  said("12"), said("spin button"), said("pt")],
                 should="the reader hears the name, the value with its unit (pt), and the role"):
        run.grab_focus(role="spin button", name="Font size")
    with run.act("Up", [said_exactly("13"), spin_value("Font size", 13, "13")],
                 should="the value goes up one step and the reader hears 13"):
        run.key("Up")
    with run.act("Down", [said_exactly("12"), spin_value("Font size", 12, "12")],
                 should="the value goes down one step and the reader hears 12"):
        run.key("Down")
    with run.act("PageUp", [said_exactly("22"), spin_value("Font size", 22, "22")],
                 should="the value goes up a page (10) and the reader hears 22"):
        run.key("PageUp")
    with run.act("PageDown", [said_exactly("12"), spin_value("Font size", 12, "12")],
                 should="the value goes down a page and the reader hears 12"):
        run.key("PageDown")
    with run.act("End", [spin_value("Font size", 12, "12"),
                         no_event("object:property-change:accessible-value")],
                 should="End belongs to the caret (docs/range-keyboard.md); the value stays 12"):
        run.key("End")
    with run.act("Home", [spin_value("Font size", 12, "12"),
                          no_event("object:property-change:accessible-value")],
                 should="Home belongs to the caret; the value stays 12"):
        run.key("Home")
    with run.act("PageUp x10 to the maximum", [said("96"), spin_value("Font size", 96, "96")],
                 should="the value clamps at 96 and the reader hears it"):
        run.key(*["PageUp"] * 10, gap=0.25)
    with run.act("Up at the maximum", [spin_value("Font size", 96, "96"), not_said("97")],
                 should="nothing changes; the reader is not told a wrong value"):
        run.key("Up")
    with run.act("PageDown x10 to the minimum", [said("4"), spin_value("Font size", 4, "4")],
                 should="the value clamps at 4 and the reader hears it"):
        run.key(*["PageDown"] * 10, gap=0.25)


def adaptive_body(run):
    """Frequency steps adaptively (Qt's AdaptiveDecimalStepType): at 440 a
    press moves by 100. What does the Value interface say the step is?"""
    setup_focus(run, "Frequency")

    def increment_matches(act):
        for node in _walk(act.tree):
            if node.get("role") == "spin button" and node.get("name") == "Frequency":
                value = node.get("value") or {}
                moved = abs((value.get("current") or 0.0) - 440.0)
                return abs((value.get("increment") or 0.0) - moved) < 1e-9, [
                    f"[spin button] 'Frequency' value={value} text={node.get('text')}; "
                    f"one Up moved it by {moved:g}"]
        return False, ["no Frequency"]
    with run.act("Frequency: Up (adaptive step)",
                 [said("540"), custom("the Value interface's minimum increment is the step "
                                      "a press just took", increment_matches, needs_tree=True)],
                 should="the value moves by the adaptive step (100 at 440) and the reader "
                        "hears 540.00"):
        run.key("Up")


# ---------------------------------------------------------------------------
# The caret inside the field
# ---------------------------------------------------------------------------


def caret_body(run):
    """Home, End, Left and Right inside a spin box: the keys the widget leaves
    to the text caret. A reader reviews the number by these."""
    setup_focus(run, "Font size")
    with run.act("Home", [caret_at("Font size", 0),
                          event("object:text-caret-moved", role="spin button")],
                 should="the caret goes to the start of 12, and the bus says so",
                 tree=True):
        run.key("Home")
    with run.act("Right", [caret_at("Font size", 1),
                           event("object:text-caret-moved", role="spin button")],
                 should="the caret moves past the 1; the bus says so", tree=True):
        run.key("Right")
    with run.act("End", [caret_at("Font size", 2),
                         event("object:text-caret-moved", role="spin button")],
                 should="the caret goes to the end of 12", tree=True):
        run.key("End")
    with run.act("Left", [caret_at("Font size", 1),
                          event("object:text-caret-moved", role="spin button")],
                 should="the caret moves one character left", tree=True):
        run.key("Left")
    with run.act("Shift+Home (select the 1)",
                 [event("object:text-selection-changed", role="spin button")],
                 should="a selection is made and the bus says so", tree=True):
        run.key("Shift+Home")
    with run.act("type 3 (replaces the selected 1)", [caret_at("Font size", 1, "32")],
                 should="the 3 replaces the selected 1", tree=True):
        run.type("3")
    with run.act("Escape", should="nothing a reader needs", tree=True):
        run.key("Escape")


# ---------------------------------------------------------------------------
# Typing
# ---------------------------------------------------------------------------


def typing_body(run):
    setup_focus(run, "Font size")
    with run.act("type 20 then Enter", [spin_value("Font size", 20, "20"), said("20")],
                 should="the typed value commits; the reader hears 20"):
        run.type("20")
        run.key("Enter")
    with run.act("select all, type 500 then Enter (max is 96)",
                 [spin_value("Font size", 96, "96"), said("96")],
                 should="the value is clamped to 96 and the reader hears the value that "
                        "was kept, not the 500 they typed"):
        run.key("Ctrl+A")
        run.type("500")
        run.key("Enter")
    with run.act("select all, type 3 then Enter (min is 4)",
                 [spin_value("Font size", 4, "4"), said("4")],
                 should="the value is clamped to 4 and the reader hears it"):
        run.key("Ctrl+A")
        run.type("3")
        run.key("Enter")
    with run.act("type a letter (Font size filters characters)",
                 [spin_value("Font size", 4, "4")],
                 should="the letter is refused; nothing changes"):
        run.key("End")
        run.type("x")
    with run.act("select all, type 30, Tab away (commit on blur)",
                 [spin_value("Font size", 30, "30"), focused(role="spin button", name="Gain"),
                  said("Gain")],
                 should="the typed value commits as focus leaves; the reader hears the next "
                        "box"):
        run.key("Ctrl+A")
        run.type("30")
        run.key("Tab")
    with run.act("Gain: select all, type abc then Enter",
                 [spin_value("Gain", 0.0, "0.0"),
                  custom("the reader is told the entry was refused (invalid state, a message, "
                         "or at least the value it went back to)", lambda act: (
                             any("invalid" in normalized(u.text) or "0.0" in u.text
                                 for u in utterances(act.orca)),
                             [f"Orca said: {u.text!r}" for u in utterances(act.orca)]
                             or ["Orca said nothing"]), needs_orca=True)],
                 should="Gain's custom parser has no character filter, so the letters go "
                        "in; Enter reverts to 0.0, and the reader should learn the entry "
                        "was refused"):
        run.key("Ctrl+A")
        run.type("abc")
        run.key("Enter")
    with run.act("Gain: select all, type -7.5 then Enter",
                 [spin_value("Gain", -7.5, "-7.5"), said("7.5")],
                 should="a negative decimal commits and the reader hears it"):
        run.key("Ctrl+A")
        run.type("-7.5")
        run.key("Enter")
    with run.act("Gain: Down (step 0.5)", [said("8.0"), spin_value("Gain", -8.0, "-8.0")],
                 should="the reader hears minus 8.0"):
        run.key("Down")
    with run.act("Gain: type 5 after the text (-8.05), then Up (steps from the typing)",
                 [custom("Gain's value is -8.05 + 0.5 = -7.55", lambda act: next(
                     ((abs(((n.get("value") or {}).get("current") or 0) + 7.55) < 1e-6,
                       [f"value={n.get('value')} text={n.get('text')}"])
                      for n in _walk(act.tree)
                      if n.get("role") == "spin button" and n.get("name") == "Gain"),
                     (False, ["no Gain"])), needs_tree=True),
                  said("7.")],
                 should="the typed -8.05 is read and stepped by 0.5; the reader hears the "
                        "new value (shown with one decimal)"):
        run.key("End")
        run.type("5")
        run.key("Up")


# ---------------------------------------------------------------------------
# Special value text: Timeout shows "Auto" at its minimum
# ---------------------------------------------------------------------------


def special_body(run):
    setup_focus(run, "Opacity")
    with run.act("Tab to Timeout (value 0 = Auto)",
                 [focused(role="spin button", name="Timeout"), said("Timeout"),
                  said("Auto")],
                 should="the reader hears that the timeout is Auto, which is what the "
                        "box means at 0 and what a sighted user saw a moment ago"):
        run.key("Tab")
    with run.act("Up", [said_exactly("1"), spin_value("Timeout", 1, "1")],
                 should="the reader hears 1"):
        run.key("Up")
    with run.act("Down back to the minimum", [said("Auto"), spin_value("Timeout", 0)],
                 should="the value returns to 0 and the reader hears Auto, "
                        "consistently with what Tab-in said"):
        run.key("Down")
    with run.act("Tab away from Timeout (text already Auto)",
                 [focused(role="spin button", name="Frequency"), said("Frequency"),
                  not_said("unselected")],
                 should="the reader hears the next box, and nothing about the box being "
                        "left"):
        run.key("Tab")
    with run.act("Shift+Tab back to Timeout", [said("Timeout"), said("Auto")],
                 should="the reader hears Timeout, Auto"):
        run.key("Shift+Tab")
    with run.act("Tab away from Timeout again (text flips 0 -> Auto)",
                 [said("Frequency"), not_said("unselected")],
                 should="no stray 'Text unselected' when the text flips back to Auto"):
        run.key("Tab")
    with run.act("Shift+Tab back to Timeout, select all, type 0, Enter",
                 [said("Auto")],
                 should="typing the minimum commits it; the box reads Auto"):
        run.key("Shift+Tab")
        run.wait(1.0)
        run.key("Ctrl+A")
        run.type("0")
        run.key("Enter")


def leave_timeout_body(run):
    """Just the flip: land on Timeout, leave it. Three times in one run."""
    setup_focus(run, "Opacity")
    for i in (1, 2, 3):
        with run.act(f"Tab to Timeout ({i})", [said("Timeout")]):
            run.key("Tab")
        with run.act(f"Tab away from Timeout ({i})",
                     [said("Frequency"), not_said("unselected"),
                      no_event("object:text-selection-changed", role="spin button",
                               name_contains="Timeout")],
                     should="the reader hears Frequency and nothing about the text Timeout "
                            "put back as it lost focus"):
            run.key("Tab")
        with run.act(f"Shift+Tab x2 back to Opacity ({i})",
                     [focused(role="spin button", name="Opacity")]):
            run.key("Shift+Tab", "Shift+Tab", gap=0.8)


# ---------------------------------------------------------------------------
# Read-only mirror, wrap mode
# ---------------------------------------------------------------------------


def readonly_body(run):
    with run.act("focus the read-only mirror (AT-SPI grab_focus)",
                 [said("Font size mirror"), said("12"), not_said("grayed"),
                  spin_state("Font size mirror", "read-only"),
                  spin_state("Font size mirror", "sensitive")],
                 should="the reader hears a read-only spin box, not a disabled one",
                 tree=True):
        run.grab_focus(role="spin button", name="Font size mirror")
    with run.act("Up on the read-only mirror", [spin_value("Font size mirror", 12, "12")],
                 should="nothing changes"):
        run.key("Up")
    with run.act("type 5, Enter on the read-only mirror",
                 [spin_value("Font size mirror", 12, "12")],
                 should="nothing changes"):
        run.type("5")
        run.key("Enter")
    setup_focus(run, "Opacity")
    with run.act("Opacity: PageUp x2 to 100", [said("100"), spin_value("Opacity", 100, "100")]):
        run.key("PageUp", "PageUp", gap=0.4)
    with run.act("Opacity: Up at 100 wraps to 0", [said_exactly("0"),
                                                   spin_value("Opacity", 0, "0")],
                 should="the value wraps to 0 and the reader hears 0"):
        run.key("Up")
    with run.act("Opacity: Down at 0 wraps to 100", [said_exactly("100"),
                                                     spin_value("Opacity", 100, "100")],
                 should="the value wraps to 100 and the reader hears 100"):
        run.key("Down")


# ---------------------------------------------------------------------------
# Scrolling away and back
# ---------------------------------------------------------------------------


def scroll_back_body(run):
    """Tab to the bottom of the page and Shift+Tab back to the top. The
    AccessKit filter drops a clipped parent's off-screen children, so the top
    boxes leave the AT-SPI tree as the page scrolls (defunct) and come back
    with the same ids as it scrolls back."""
    setup_focus(run, "Font size")
    tab_until(run, "spin button", "Port")
    with run.act("Shift+Tab to Population, ungrouped",
                 [focused(role="spin button", name="Population, ungrouped"),
                  said("Population, ungrouped")]):
        run.key("Shift+Tab")
    tab_until(run, "spin button", "Frequency", chord="Shift+Tab")
    with run.act("Shift+Tab to Timeout (it left the tree and came back)",
                 [focused(role="spin button", name="Timeout"), said("Timeout")],
                 should="the reader hears Timeout, as on the way down"):
        run.key("Shift+Tab")
    with run.act("Up on Timeout", [said_exactly("1"), spin_value("Timeout", 1, "1")],
                 should="the reader hears 1"):
        run.key("Up")
    with run.act("Shift+Tab to Opacity", [focused(role="spin button", name="Opacity"),
                                          said("Opacity")]):
        run.key("Shift+Tab")
    with run.act("Shift+Tab to Gain", [focused(role="spin button", name="Gain"), said("Gain")]):
        run.key("Shift+Tab")
    with run.act("Shift+Tab to Font size", [focused(role="spin button", name="Font size"),
                                            said("Font size"),
                                            spin_state("Font size", "defunct", present=False)],
                 tree=True):
        run.key("Shift+Tab")
    with run.act("Up on Font size", [said_exactly("13"), spin_value("Font size", 13, "13")],
                 should="the reader hears 13"):
        run.key("Up")


# ---------------------------------------------------------------------------
# Locale
# ---------------------------------------------------------------------------


def locale_body(run):
    setup_focus(run, "Language", role="combo box")
    with run.act("Language: Down, Enter picks français",
                 [spin_value("Gain", 0.0, "0,0"), spin_value("Frequency", 440.0, "440,00")],
                 should="the language becomes French and every box re-renders its number",
                 tree=True):
        run.key("Down")
        run.wait(1.0)
        run.key("Enter")
    setup_focus(run, "Gain")
    with run.act("Gain: Down in French", [said("0,5"), spin_value("Gain", -0.5, "-0,5")],
                 should="the reader hears minus 0,5, as shown"):
        run.key("Down")
    setup_focus(run, "Frequency")
    with run.act("Frequency: select all, type 12,5 then Enter",
                 [spin_value("Frequency", 12.5, "12,50"), said("12,5")],
                 should="the French decimal commits; the reader hears 12,50"):
        run.key("Ctrl+A")
        run.type("12,5")
        run.key("Enter")
    tab_until(run, "spin button", "Opacity (fill)")
    with run.act("Tab to Population (grouped) in French",
                 [focused(role="spin button", name="Population"), said("Population"),
                  said("567"), spin_value("Population", 1234567, "1\u202f234\u202f567")],
                 should="the reader hears the population grouped as French groups it "
                        "(a narrow no-break space), as the box showed before focus",
                 tree=True):
        run.key("Tab")
    with run.act("Tab, Tab to Port (not localized) in French", [said("Port"), said("8080")]):
        run.key("Tab", "Tab", gap=1.0)


# ---------------------------------------------------------------------------
# What an assistive technology other than a keyboard does: AT-SPI Value
# ---------------------------------------------------------------------------


def _atspi_node(name: str):
    """The spin button `name` on the private bus, looked up from this process.

    The driver runs inside the private session (the harness refuses anything
    else), so this is the private AT-SPI bus, never the desktop's.
    """
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    desktop = Atspi.get_desktop(0)
    stack = []
    for i in range(desktop.get_child_count()):
        app = desktop.get_child_at_index(i)
        if app is not None and app.get_name() == "spin-box":
            stack.append(app)
    while stack:
        node = stack.pop()
        try:
            if node.get_role_name() == "spin button" and node.get_name() == name:
                return Atspi, node
            for i in range(node.get_child_count()):
                child = node.get_child_at_index(i)
                if child is not None:
                    stack.append(child)
        except Exception:
            continue
    raise RunError(f"no spin button {name!r} on the private bus")


def atspi_value_body(run):
    setup_focus(run, "Font size")
    with run.act("AT-SPI Value.SetCurrentValue(30) on Font size",
                 [spin_value("Font size", 30, "30"), said("30")],
                 should="the value becomes 30 and, since it is focused, the reader hears it"):
        Atspi, node = _atspi_node("Font size")
        run._step("AT-SPI Value.set_current_value(30)")
        Atspi.Value.set_current_value(node, 30.0)
    with run.act("AT-SPI Value.SetCurrentValue(500) on Font size (max 96)",
                 [spin_value("Font size", 96, "96"), said("96")],
                 should="the value clamps to 96 and the reader hears it"):
        Atspi, node = _atspi_node("Font size")
        run._step("AT-SPI Value.set_current_value(500)")
        Atspi.Value.set_current_value(node, 500.0)
    with run.act("AT-SPI EditableText.SetTextContents('40') on Font size",
                 [spin_value("Font size", 40, "40")],
                 should="the string takes Enter's parse; the value becomes 40"):
        Atspi, node = _atspi_node("Font size")
        run._step("AT-SPI EditableText.set_text_contents('40')")
        ok = Atspi.EditableText.set_text_contents(node, "40")
        run._step(f"returned {ok}")
    with run.act("AT-SPI Value.SetCurrentValue(20) on the read-only mirror",
                 [spin_value("Font size mirror", 40, "40")],
                 should="a read-only box refuses the write"):
        Atspi, node = _atspi_node("Font size mirror")
        run._step("AT-SPI Value.set_current_value(20) on the read-only mirror")
        ok = Atspi.Value.set_current_value(node, 20.0)
        run._step(f"returned {ok}")


SCENARIOS = [
    Scenario("spinbox-tree", "spin-box", tree_body,
             "the spin boxes on the bus: roles, values, units, what sits between them"),
    Scenario("spinbox-steps", "spin-box", steps_body,
             "Font size: Up/Down/PageUp/PageDown/Home/End, the maximum and the minimum"),
    Scenario("spinbox-adaptive", "spin-box", adaptive_body,
             "Frequency's adaptive step against the step the Value interface publishes"),
    Scenario("spinbox-caret", "spin-box", caret_body,
             "Home/End/Left/Right/Shift+Home inside Font size: does the caret reach the bus"),
    Scenario("spinbox-typing", "spin-box", typing_body,
             "typing into Font size and Gain: commit, clamp, filter, refuse, commit on blur"),
    Scenario("spinbox-special", "spin-box", special_body,
             "Timeout's special value text 'Auto' and leaving the box"),
    Scenario("spinbox-leave-timeout", "spin-box", leave_timeout_body,
             "leaving Timeout three times: the 'Text unselected.' Orca says as Auto comes back"),
    Scenario("spinbox-readonly", "spin-box", readonly_body,
             "the read-only mirror, and Opacity's wrap mode"),
    Scenario("spinbox-scroll-back", "spin-box", scroll_back_body,
             "Tab to the bottom and Shift+Tab back: boxes that left the tree and came back"),
    Scenario("spinbox-locale", "spin-box", locale_body,
             "switching to French: decimal separator, grouping, the port"),
    Scenario("spinbox-atspi-value", "spin-box", atspi_value_body,
             "setting a value through AT-SPI Value / EditableText, as a non-keyboard AT does"),
]
