# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""new-widgets-kit: Banner in a Collapse, SearchField, InputDialog,
FilePickerField and CommandLinkButton, as a reader meets them.

The example (`examples/new_widgets_kit/src/main.rs`) stacks, top to bottom:

* three `Banner`s, each wrapped in a `Collapse` bound to a `Signal<bool>`
  (`banner_section`). "Welcome to Teksilo" (info, no dismiss), "Unsaved
  changes" (warning, a "Save now" action and a dismiss button) and "Disk
  almost full" (error, a dismiss button). A dismiss sets its signal false,
  and the Collapse animates the banner's height to zero; a ghost "Restore
  banners" button sets all three true again. A `Banner` is a `Role::Status`
  / `Live::Polite` node named by its title (`banner.rs`,
  `Widget::accessibility`); its dismiss button is `IconButton::clear()`,
  whose name is the built-in "Clear" tooltip. `Collapse` publishes nothing
  and never hides its child from assistive technology
  (`animations/collapse.rs`, `accessibility` is empty; its only reaction to
  `expanded` is a height tween);
* a `SearchField` with a placeholder and a prefix-matched fruit suggestion
  provider, and two readout labels under it (not live regions);
* a `FilePickerField` (entry + "Browse");
* a "Rename…" button that presents an `InputDialog` ("Rename document",
  prompt "Enter the new file name:", default "untitled.txt", placeholder
  "filename.ext", no validator), and two readout labels;
* two `CommandLinkButton`s in a `Card`.

Initial focus lands on the search field. The Tab order is: search, file
entry, Browse, Rename…, the two command links, the Theme combo box in the
toolbar, Save now, Clear, Clear, Restore banners.

None of the example's own messages goes through `ctx.announce`: the only
speech that is not a focus reading comes from the three banners' own
`Live::Polite` nodes.
"""

from __future__ import annotations

from reader_lib.checks import (announced, custom, event, focused, in_tree, no_event,
                               not_announced, not_in_tree, not_said, said, said_once)
from reader_lib.orca import utterances
from reader_lib.scenario import Scenario

PACKAGE = "new-widgets-kit"


# ---------------------------------------------------------------------------
# Tree helpers
# ---------------------------------------------------------------------------


def _walk(tree):
    if not tree:
        return
    stack = [tree]
    while stack:
        node = stack.pop()
        yield node
        stack.extend(reversed(node.get("children", [])))


def _path_to(tree, pred):
    if not tree:
        return None
    if pred(tree):
        return [tree]
    for child in tree.get("children", []):
        found = _path_to(child, pred)
        if found:
            return [tree] + found
    return None


def _outline(node) -> str:
    return f"[{node.get('role')}] {node.get('name')!r} ext={node.get('extents')}"


def _is_focus(e):
    if e["type"] == "object:state-changed:focused":
        return e.get("detail1") == 1
    return e["type"] == "object:active-descendant-changed"


def _focus_target(e):
    return e.get("target", {}) if e["type"] == "object:active-descendant-changed" \
        else e.get("source", {})


def _last_focus_in_history(act):
    moves = [e for e in act.history if e["type"] == "object:state-changed:focused"
             and e.get("detail1") == 1]
    return moves[-1].get("source", {}) if moves else {}


def _focused_node(act):
    target = _last_focus_in_history(act)
    return next((n for n in _walk(act.tree) if n.get("path") == target.get("path")), None)


def focus_not_inside(role: str, name: str):
    """After the act, the node that holds focus (the last focus change the bus
    carried, this act or before) does not sit inside [role] name."""
    def run(act):
        target = _last_focus_in_history(act)
        path = _path_to(act.tree, lambda n: n.get("path") == target.get("path"))
        if not path:
            return True, [f"focus holder {target.get('role')} {target.get('name')!r} "
                          "is not in the tree"]
        inside = [n for n in path if n.get("role") == role and (n.get("name") or "") == name]
        return not inside, ["focus path: " + " > ".join(_outline(n) for n in path)]
    return custom(f"focus is not left inside [{role}] {name!r}", run, needs_tree=True)


def subtree_outline(role: str, name: str):
    """Record what is left of [role] name in the tree (an always-passing
    probe, for the report's evidence)."""
    def run(act):
        node = next((n for n in _walk(act.tree) if n.get("role") == role
                     and (n.get("name") or "") == name), None)
        if node is None:
            return True, [f"no [{role}] {name!r}"]
        return True, [_outline(n) + f" states={n.get('states')}" for n in _walk(node)]
    return custom(f"(probe) what is left of [{role}] {name!r}", run, needs_tree=True)


def orca_said_any(*texts: str):
    """Orca said at least one of `texts`, uncut."""
    def run(act):
        spoken = [u for u in utterances(act.orca) if not u.cut]
        hit = [u.text for u in spoken if any(t.casefold() in u.text.casefold() for t in texts)]
        return bool(hit), [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                           for u in utterances(act.orca)] or ["Orca said nothing"]
    return custom(f"Orca says one of {list(texts)}", run, needs_orca=True)


def orca_said_something():
    """Orca said anything at all in the act, uncut."""
    def run(act):
        spoken = [u for u in utterances(act.orca) if not u.cut]
        return bool(spoken), [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                              for u in utterances(act.orca)] or ["Orca said nothing"]
    return custom("Orca says something", run, needs_orca=True)


def focused_name_says(*words: str):
    """The focused node's name contains one of `words`."""
    def run(act):
        node = _focused_node(act)
        if node is None:
            return False, ["the focus holder is not in the tree"]
        name = node.get("name") or ""
        ok = any(w.casefold() in name.casefold() for w in words)
        return ok, [_outline(node) + f" desc={node.get('description')!r}"]
    return custom(f"the focused control's name says one of {list(words)}", run,
                  needs_tree=True)


def field_named(role: str = "entry"):
    """The focused [role] node has a name."""
    def run(act):
        node = _focused_node(act)
        if node is None:
            return False, ["the focus holder is not in the tree"]
        attrs = node.get("attributes") or {}
        ok = bool((node.get("name") or "").strip()) and node.get("role") == role
        return ok, [_outline(node) + f" desc={node.get('description')!r} attrs={attrs} "
                    f"rel={node.get('relations')} text={node.get('text')}"]
    return custom(f"the focused [{role}] has a name", run, needs_tree=True)


def focus_holder_is(role: str, name: str | None = None):
    """The node holding focus after the act (the last focus change the bus
    carried, in this act or before) is [role] name: for an act that should
    leave focus where it was."""
    def run(act):
        target = _last_focus_in_history(act)
        ok = target.get("role") == role and (name is None or target.get("name") == name)
        return ok, [f"focus holder [{target.get('role')}] {target.get('name')!r}"]
    return custom(f"focus stays on [{role}] {name if name is not None else '*'!r}", run)


def focused_has_state(state: str):
    def run(act):
        node = _focused_node(act)
        if node is None:
            return False, ["the focus holder is not in the tree"]
        return state in (node.get("states") or []), [
            _outline(node) + f" states={node.get('states')} attrs={node.get('attributes')} "
            f"rel={node.get('relations')}"]
    return custom(f"the focused node has state {state!r}", run, needs_tree=True)


def focused_text_is(text: str):
    def run(act):
        node = _focused_node(act)
        if node is None:
            return False, ["the focus holder is not in the tree"]
        got = (node.get("text") or {}).get("text")
        return got == text, [_outline(node) + f" text={got!r}"]
    return custom(f"the focused node's text is {text!r}", run, needs_tree=True)


def search_field_nodes():
    """(probe) every node of the SearchField: the outer [entry] that has
    children, and what is under it."""
    def run(act):
        outer = next((x for x in _walk(act.tree) if x.get("role") == "entry"
                      and x.get("children")), None)
        if outer is None:
            return True, ["no entry with children"]
        return True, [_outline(n) + f" states={n.get('states')} attrs={n.get('attributes')} "
                      f"rel={n.get('relations')} ifs={n.get('interfaces')} "
                      f"text={(n.get('text') or {}).get('text')!r}"
                      for n in _walk(outer)]
    return custom("(probe) the SearchField's nodes", run, needs_tree=True)


def search_clear_reachable():
    """The SearchField's clear affordance is a node a reader can find and
    activate: a push button inside the field's subtree."""
    def run(act):
        outer = next((x for x in _walk(act.tree) if x.get("role") == "entry"
                      and x.get("children")), None)
        if outer is None:
            return False, ["no entry with children"]
        buttons = [n for n in _walk(outer) if n.get("role") == "push button"]
        return bool(buttons), [_outline(n) for n in _walk(outer)]
    return custom("the SearchField's clear button is in the tree, inside the field", run,
                  needs_tree=True)


def expanded_on_focus_holder():
    """An `expanded` state change reached the bus from the node holding focus."""
    def run(act):
        target = _last_focus_in_history(act)
        evs = [e for e in act.events if e["type"] == "object:state-changed:expanded"]
        on_focus = [e for e in evs if e.get("source", {}).get("path") == target.get("path")]
        lines = [f"{e['type']} {e.get('detail1')} from {e.get('source', {}).get('path')}"
                 for e in evs] or ["no object:state-changed:expanded in the act"]
        lines.append(f"focus holder path {target.get('path')}")
        return bool(on_focus), lines
    return custom("the focused field says it expanded (state-changed:expanded)", run)


# ---------------------------------------------------------------------------
# Banners in a Collapse
# ---------------------------------------------------------------------------


def banner_dismiss_keys(run):
    """Dismiss "Unsaved changes" with real keys, Shift+Tab on, restore."""
    run.wait_for(role="entry", state="focused")
    with run.act("Shift+Tab three times to the Unsaved changes dismiss button",
                 [focused(role="push button", name="Clear"), said("Unsaved changes"),
                  focused_name_says("dismiss", "close")],
                 should="focus reaches the warning banner's dismiss button; the reader "
                        "hears the banner and a button that says it dismisses it",
                 tree=True):
        run.key("Shift+Tab", "Shift+Tab", "Shift+Tab")
    with run.act("Space on the dismiss button",
                 [not_in_tree(role="status bar", name="Unsaved changes"),
                  focus_not_inside("status bar", "Unsaved changes"),
                  orca_said_something(),
                  subtree_outline("status bar", "Unsaved changes")],
                 should="the banner goes away for the reader as for the eye, and focus "
                        "moves to a control that is still there, which the reader hears",
                 tree=True, record=3.0):
        run.key("space")
    with run.act("Shift+Tab after the dismiss",
                 [focus_not_inside("status bar", "Unsaved changes"),
                  not_said("Save now")],
                 should="Shift+Tab goes back to the previous control a sighted user sees "
                        "(the Theme combo box), not into the dismissed banner",
                 tree=True):
        run.key("Shift+Tab")
    with run.act("Activate Restore banners through AT-SPI",
                 [in_tree(role="status bar", name="Unsaved changes"),
                  announced("Unsaved changes"), said("Unsaved changes")],
                 should="the banner comes back, and as a polite live region it is "
                        "announced when it appears",
                 tree=True, record=3.0):
        run.action("click", role="push button", name="Restore banners")


def banner_dismiss_at(run):
    """Dismiss "Disk almost full" as a screen reader's activation does, then
    look at the Tab order around it."""
    run.wait_for(role="entry", state="focused")
    with run.act("AT-SPI click on the Disk almost full dismiss button",
                 [not_in_tree(role="status bar", name="Disk almost full"),
                  subtree_outline("status bar", "Disk almost full")],
                 should="the banner leaves the tree a reader walks",
                 tree=True, record=3.0):
        run.action("click", role="push button", name="Clear", nth=1)
    with run.act("Shift+Tab from the search field after the dismiss",
                 [focused(role="push button", name="Restore banners")],
                 should="focus goes to Restore banners"):
        run.key("Shift+Tab")
    with run.act("Shift+Tab again",
                 [focus_not_inside("status bar", "Disk almost full"),
                  not_said("Disk almost full")],
                 should="focus skips the dismissed banner's button (a sighted keyboard "
                        "user sees nothing there) and reaches the warning banner's "
                        "dismiss button",
                 tree=True):
        run.key("Shift+Tab")


# ---------------------------------------------------------------------------
# SearchField
# ---------------------------------------------------------------------------


def search_suggestions(run):
    run.wait_for(role="entry", state="focused")
    with run.act("type 'ap' in the search field",
                 [in_tree(role="list box"), in_tree(role="list item", name="Apple"),
                  expanded_on_focus_holder(), focused_has_state("expanded"),
                  orca_said_any("suggestion", "Apple", "expanded", "list"),
                  search_field_nodes()],
                 should="the suggestion list opens under the field and the reader learns "
                        "suggestions are there to arrow into",
                 tree=True, record=3.0):
        run.type("ap")
    with run.act("Down into the suggestions",
                 [event("object:active-descendant-changed"), said("Apple"),
                  in_tree(role="list item", name="Apple", state="selected")],
                 should="the first suggestion is highlighted (selected in the tree) and the "
                        "reader hears 'Apple'", tree=True):
        run.key("Down")
    with run.act("Down again",
                 [event("object:active-descendant-changed"), said("Apricot"),
                  in_tree(role="list item", name="Apricot", state="selected")],
                 should="the reader hears 'Apricot'", tree=True):
        run.key("Down")
    with run.act("Enter picks the highlighted suggestion",
                 [not_in_tree(role="list box"),
                  in_tree(role="label", name='Filtering: "Apricot"')],
                 should="the field takes 'Apricot' and the list closes",
                 tree=True, record=3.0):
        run.key("Enter")
    with run.act("Enter again submits the query",
                 [said("Submitted")],
                 should="the example's 'Submitted 1 time(s).' status line reaches the "
                        "reader, as a status message should (WCAG 4.1.3)"):
        run.key("Enter")
    with run.act("Shift+Tab away and Tab back to the field",
                 [focused(role="entry"), field_named("entry"), said("Apricot")],
                 should="back on the field the reader hears its name and its value",
                 tree=True):
        run.key("Shift+Tab")
        run.wait(1.0)
        run.key("Tab")


def search_escape(run):
    run.wait_for(role="entry", state="focused")
    with run.act("type 'b'", [in_tree(role="list item", name="Banana")],
                 should="the list opens with Banana, Blackberry, Blueberry",
                 tree=True, record=3.0):
        run.type("b")
    with run.act("Up wraps to the last suggestion",
                 [event("object:active-descendant-changed"), said("Blueberry")],
                 should="the reader hears 'Blueberry'"):
        run.key("Up")
    with run.act("Escape closes the list",
                 [not_in_tree(role="list box"), focus_holder_is("entry"),
                  in_tree(role="label", name='Filtering: "b"'),
                  search_clear_reachable(), search_field_nodes()],
                 should="the list closes, focus stays on the field, the typed 'b' stays; "
                        "the field's visible clear (X) button is a control a reader can "
                        "find",
                 tree=True):
        run.key("Escape")
    with run.act("Ctrl+A, BackSpace empties the field",
                 [event("object:text-changed:delete", role="entry", text_contains="b")],
                 should="the reader's text-change events say the 'b' was deleted"):
        run.key("Ctrl+A", "BackSpace")
    with run.act("type 'c' into the empty field",
                 [event("object:text-changed:insert", role="entry", text_contains="c")],
                 should="the first character typed into an empty field is reported as "
                        "inserted, as every later one is"):
        run.type("c")
    with run.act("type 'h' after it",
                 [event("object:text-changed:insert", role="entry", text_contains="h")],
                 should="the second character is reported as inserted (the control case)"):
        run.type("h")


# ---------------------------------------------------------------------------
# InputDialog
# ---------------------------------------------------------------------------


def dialog_count():
    """(probe) how many dialog nodes the tree holds, and around the field."""
    def run(act):
        dialogs = [n for n in _walk(act.tree) if n.get("role") in ("dialog", "alert")]
        path = _path_to(act.tree, lambda n: n.get("role") == "entry"
                        and "focused" in (n.get("states") or []))
        return len(dialogs) == 1, [f"dialogs: {[_outline(d) for d in dialogs]}",
                                   "focus path: " + (" > ".join(_outline(n) for n in path)
                                                     if path else "none")]
    return custom("exactly one dialog node", run, needs_tree=True)


def dialog_outline():
    def run(act):
        dlg = next((n for n in _walk(act.tree) if n.get("role") == "dialog"), None)
        if dlg is None:
            return True, ["no dialog"]
        return True, [_outline(n) + f" states={n.get('states')} desc={n.get('description')!r} "
                      f"attrs={n.get('attributes')} rel={n.get('relations')}"
                      for n in _walk(dlg)]
    return custom("(probe) the dialog's nodes", run, needs_tree=True)


def _open_rename(run, label="Space on Rename… opens the InputDialog", extra=None):
    run.wait_for(role="entry", state="focused")
    with run.act("Tab three times to Rename…",
                 [focused(role="push button", name="Rename…")],
                 should="focus reaches the Rename… button"):
        run.key("Tab", "Tab", "Tab")
    expect = [in_tree(role="dialog", name="Rename document"),
              dialog_count(),
              focused(role="entry"),
              said("Rename document"),
              said("Enter the new file name"),
              field_named("entry"),
              dialog_outline()]
    with run.act(label, expect + list(extra or []),
                 should="one dialog named 'Rename document' opens with focus in its field; "
                        "the reader hears the dialog, the prompt as the field's name, and "
                        "the field's value",
                 tree=True, record=3.5):
        run.key("space")


def input_dialog_accept(run):
    _open_rename(run)
    with run.act("Tab to the next control in the dialog",
                 [focused(role="push button", name="Cancel"), said("Cancel")],
                 should="focus moves to Cancel"):
        run.key("Tab")
    with run.act("Tab again",
                 [focused(role="push button", name="OK"), said("OK")],
                 should="focus moves to OK"):
        run.key("Tab")
    with run.act("Tab wraps to the field",
                 [focused(role="entry"), field_named("entry"),
                  said("Enter the new file name")],
                 should="focus stays inside the modal and wraps to the field, which the "
                        "reader hears by its name (the prompt) and value",
                 tree=True):
        run.key("Tab")
    with run.act("Ctrl+A and type a new name",
                 [event("object:text-changed"), focused_text_is("notes.md")],
                 should="the field's text is replaced", tree=True):
        run.key("Ctrl+A")
        run.type("notes.md")
    with run.act("Enter accepts",
                 [not_in_tree(role="dialog"), focused(role="push button", name="Rename…"),
                  said("Rename"), in_tree(role="label", name="Current name: notes.md")],
                 should="the dialog closes, focus returns to Rename…, the reader hears it; "
                        "the new name shows in the label",
                 tree=True, record=3.0):
        run.key("Enter")


def input_dialog_escape(run):
    _open_rename(run)
    with run.act("Escape cancels",
                 [not_in_tree(role="dialog"), focused(role="push button", name="Rename…"),
                  said("Rename"), in_tree(role="label", name_contains="cancelled")],
                 should="the dialog closes, focus returns to Rename… and the reader hears it",
                 tree=True, record=3.0):
        run.key("Escape")
    with run.act("Open again with Space", [in_tree(role="dialog", name="Rename document"),
                                           dialog_count(),
                                           focused(role="entry"), said("Rename document")],
                 should="the dialog opens again and is heard again",
                 tree=True, record=3.5):
        run.key("space")
    with run.act("Cancel through AT-SPI",
                 [not_in_tree(role="dialog"), focused(role="push button", name="Rename…"),
                  said("Rename")],
                 should="a screen reader's activation of Cancel closes the dialog and "
                        "focus returns to Rename…",
                 tree=True, record=3.0):
        run.action("click", role="push button", name="Cancel")
    with run.act("Open through an AT-SPI click on Rename…",
                 [in_tree(role="dialog", name="Rename document"), dialog_count(),
                  focused(role="entry"), said("Rename document")],
                 should="a screen reader's own activation of Rename… opens the same one "
                        "dialog with focus in its field",
                 tree=True, record=3.5):
        run.action("click", role="push button", name="Rename…")
    with run.act("Escape from the AT-opened dialog",
                 [not_in_tree(role="dialog"), focused(role="push button", name="Rename…"),
                  said("Rename")],
                 should="the dialog closes and focus returns to Rename…",
                 tree=True, record=3.0):
        run.key("Escape")


# ---------------------------------------------------------------------------
# Registry
# ---------------------------------------------------------------------------


SCENARIOS = [
    Scenario("newkit-banner-dismiss-keys", PACKAGE, banner_dismiss_keys,
             "dismiss the warning banner with keys, Shift+Tab on, restore the banners"),
    Scenario("newkit-banner-dismiss-at", PACKAGE, banner_dismiss_at,
             "dismiss the error banner through AT-SPI, then Shift+Tab around it"),
    Scenario("newkit-search-suggestions", PACKAGE, search_suggestions,
             "type into the SearchField, arrow through suggestions, pick, submit"),
    Scenario("newkit-search-escape", PACKAGE, search_escape,
             "type into the SearchField, Up to the last suggestion, Escape"),
    Scenario("newkit-dialog-accept", PACKAGE, input_dialog_accept,
             "open the InputDialog, Tab around it, type a name, Enter"),
    Scenario("newkit-dialog-escape", PACKAGE, input_dialog_escape,
             "open the InputDialog, Escape; open again, Cancel through AT-SPI"),
]
