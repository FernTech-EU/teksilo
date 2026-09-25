# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""menus-and-dropdowns: the menu bar, its menus, check and radio items,
submenus, type-ahead, combo boxes (one with 10 000 items) and context menus,
as a screen reader user meets them.

Where each piece of accessibility comes from:

* `MenuBarTrigger` (`crates/teksilo-widgets/src/menu_bar/trigger.rs`) is a
  `Role::MenuItem` with `has_popup(Menu)` and `expanded`, focusable; F10 and a
  bare Alt tap focus the first one, Alt+<letter> opens the matching menu
  (`menu_bar.rs`, `MenuBarDispatcher`).
* `MenuList` (`menu_list.rs`) is the unnamed `Role::Menu` that takes focus
  when a menu opens (`menu_context.rs`, `open_at` -> `request_focus`). Arrows,
  Home/End, PageUp/PageDown and type-ahead move a private `focused_index`
  signal that paints a highlight and nothing else: no focus move, no active
  descendant (`menu_list.rs`, the `on_key` handler).
* `MenuItem` (`menu_item/widget_impl.rs`, `accessibility`) gives the role by
  mode, `toggled`, and for radio items `push_to_radio_group`, which no adapter
  reads.
* `ComboBox` (`combo_box.rs`, `accessibility`) carries its selection as a
  string `value` and its placeholder as `placeholder`; the dropdown is a
  `Role::ListBox` of `Role::ListBoxOption`s (`combo_box/panel.rs`, `item.rs`).
  A plain combo keeps focus on the box while its arrows change the selection.
* Every menu and dropdown keeps its widget ids, and so its AccessKit node ids,
  from one opening to the next: closing it removes the nodes (the adapter
  sends `defunct`), reopening re-adds the same ids.

The example prints each command it runs (`println!("Copy")`, ...) to its
stdout, which the harness keeps as `app.log`; `printed` reads it to know
whether an activation reached the application, without the bridge.
"""

from __future__ import annotations

from reader_lib.checks import (Check, custom, event, focused, in_tree, no_event,
                               not_in_tree, said, _walk)
from reader_lib.orca import normalized, utterances
from reader_lib.scenario import Scenario

PKG = "menus-and-dropdowns"

# Combo boxes in walk order (the listener's `nth` counts matches in tree order,
# and the combo section is on screen at launch):
# 0 Theme (toolbar), 1 Fruit, 2 Color, 3 Size (disabled), 4 Country, 5 Huge.
COMBO = {"theme": 0, "fruit": 1, "color": 2, "size": 3, "country": 4, "huge": 5}


# ---------------------------------------------------------------------------
# Checks of our own
# ---------------------------------------------------------------------------


def spoke_anything() -> Check:
    """Orca said something in the act."""
    def run(act):
        heard = utterances(act.orca)
        return bool(heard), [f"Orca said: {u.text!r}" for u in heard] or \
            ["Orca said nothing in this act"]
    return custom("Orca says something", run, needs_orca=True)


def said_any(*texts: str) -> Check:
    """Orca said at least one of `texts`."""
    def run(act):
        heard = utterances(act.orca)
        hit = [u for u in heard for t in texts if normalized(t) in normalized(u.text)]
        return bool(hit), [f"Orca said: {u.text!r}" for u in heard] or \
            ["Orca said nothing in this act"]
    return custom(f"Orca says one of {texts!r}", run, needs_orca=True)


def _nodes(act, role=None, name=None):
    return [n for n in _walk(act.tree)
            if (role is None or n.get("role") == role)
            and (name is None or (n.get("name") or "") == name)]


def node_facts(role: str, name: str, describe: str, pred) -> Check:
    """After the act, the first [role] name node satisfies `pred(node)`."""
    def run(act):
        nodes = _nodes(act, role, name)
        if not nodes:
            return False, [f"no [{role}] {name!r} in the tree after the act"]
        n = nodes[0]
        return bool(pred(n)), [f"[{role}] {name!r} states={n.get('states')} "
                               f"attributes={n.get('attributes', {})} "
                               f"relations={n.get('relations', {})}"]
    return custom(describe, run, needs_tree=True)


def has_state(role, name, state):
    return node_facts(role, name, f"[{role}] {name!r} is {state}",
                      lambda n: state in n.get("states", []))


def lacks_state(role, name, state):
    return node_facts(role, name, f"[{role}] {name!r} is not {state}",
                      lambda n: state not in n.get("states", []))


def attr_is(role, name, attr, want):
    return node_facts(role, name, f"[{role}] {name!r} has {attr}={want}",
                      lambda n: n.get("attributes", {}).get(attr) == want)


def has_relation(role, name, relation):
    return node_facts(role, name, f"[{role}] {name!r} has a {relation} relation",
                      lambda n: relation in n.get("relations", {}))


def count_of(role: str, describe: str, pred) -> Check:
    def run(act):
        nodes = _nodes(act, role)
        sample = [f"[{n.get('role')}] {n.get('name')!r} {n.get('states')} "
                  f"{n.get('attributes', {})}" for n in nodes[:5]]
        return bool(pred(len(nodes))), [f"{len(nodes)} [{role}] nodes in the tree"] + sample
    return custom(describe, run, needs_tree=True)


def menus_open(n: int) -> Check:
    def run(act):
        menus = _nodes(act, "menu")
        return len(menus) == n, [f"{len(menus)} [menu] in the tree after the act: " + "; ".join(
            f"{m.get('name')!r} {m.get('states')} "
            f"{[c.get('name') for c in m.get('children', [])][:4]}" for m in menus)]
    return custom(f"{n} menu(s) in the tree after the act", run, needs_tree=True)


def open_menu_live() -> Check:
    """The menu in the tree after the act is not defunct to libatspi."""
    def run(act):
        menus = _nodes(act, "menu")
        if not menus:
            return False, ["no [menu] in the tree after the act"]
        return all("defunct" not in m.get("states", []) for m in menus), [
            f"[menu] {m.get('name')!r} states={m.get('states')}; first items: "
            + ", ".join(f"{c.get('name')!r} {c.get('states')}"
                        for c in m.get("children", [])[:3]) for m in menus]
    return custom("the open menu is live (not defunct) to libatspi", run, needs_tree=True)


class Printed:
    """What the example printed to its stdout during one act: the commands it
    ran. `mark()` before the act, `done()` after it; the check reads the lines
    in between."""

    def __init__(self, run):
        self.run = run
        self.start = 0
        self.lines: list[str] | None = None

    def _all(self) -> list[str]:
        try:
            return self.run.app_log.read_text(errors="replace").splitlines()
        except OSError:
            return []

    def mark(self) -> "Printed":
        self.start = len(self._all())
        return self

    def done(self) -> None:
        self.lines = [l for l in self._all()[self.start:] if l.strip()]

    def check(self, text: str) -> Check:
        def run(act):
            lines = self.lines or []
            return text in lines, [f"the example printed {lines!r} during the act"]
        return custom(f"the example ran {text!r}", run)

    def check_nothing(self) -> Check:
        def run(act):
            lines = self.lines or []
            return not lines, [f"the example printed {lines!r} during the act"]
        return custom("the example ran no command", run)


def printed(run) -> Printed:
    return Printed(run).mark()


def focus_combo(run, which: str, expect: list | None = None, should: str = "") -> None:
    """Put focus on a combo box through AT-SPI, as its own act, so what Orca
    says about it is not credited to the act after it."""
    with run.act(f"focus the {which} combo box (AT-SPI grab_focus)",
                 [focused(role="combo box"), *(expect or [])],
                 should=should or f"focus on the {which} combo box"):
        run.grab_focus(role="combo box", nth=COMBO[which])


# ---------------------------------------------------------------------------
# The menu bar through its own keys
# ---------------------------------------------------------------------------


def menubar_f10(run):
    """F10, then the File menu by keyboard, arrows through it, Escape back."""
    run.wait_for(role="menu item", name="File")
    with run.act("F10", [focused(role="menu item", name="File"), said("File")],
                 should="focus moves to the first menu bar item; the reader says 'File' "
                 "and that it opens a menu"):
        run.key("F10")
    with run.act("Down on File opens the File menu",
                 [focused(role="menu"), said_any("File menu", "New"),
                  in_tree(role="menu", name="File"), open_menu_live()],
                 should="the File menu opens; the reader hears 'File menu' and the "
                 "highlighted first item, 'New'", tree=True):
        run.key("Down")
    with run.act("Down: highlight to Open",
                 [said("Open"), focused(name="Open")],
                 should="the highlight moves to Open and the reader says 'Open'"):
        run.key("Down")
    with run.act("Down: highlight to Save", [said("Save"), focused(name="Save")],
                 should="the reader says 'Save'"):
        run.key("Down")
    with run.act("End: highlight to Quit", [said("Quit"), focused(name="Quit")],
                 should="the reader says 'Quit'"):
        run.key("End")
    with run.act("Escape closes the menu",
                 [focused(role="menu item", name="File"), said("File")],
                 should="the menu closes, focus is back on File, the reader says 'File'",
                 tree=True):
        run.key("Escape")
    with run.act("Escape on the menu bar",
                 [focused(role="frame")],
                 should="focus leaves the menu bar for where it was before F10 (the "
                 "window), as Escape does on a Windows or GTK menu bar"):
        run.key("Escape")


def f10_escape(run):
    """F10 from a combo box, then Escape: back to the combo box?"""
    run.wait_for(role="combo box", nth=COMBO["color"])
    focus_combo(run, "color")
    with run.act("F10 from the Color combo box", [focused(role="menu item", name="File")],
                 should="focus moves to File"):
        run.key("F10")
    with run.act("Escape on the menu bar",
                 [focused(role="combo box"), said("combo box")],
                 should="focus returns to the Color combo box, where the reader was"):
        run.key("Escape")
    with run.act("Tab", [focused(role="combo box")],
                 should="Tab goes on from the Color combo box to the next combo box"):
        run.key("Tab")


def menubar_alt(run):
    """Alt+F from content, Escape twice, bare Alt, Alt+E, a mnemonic. Every
    menu is opened once only, so nothing here meets a reopened menu."""
    run.wait_for(role="combo box", nth=COMBO["fruit"])
    focus_combo(run, "fruit")
    with run.act("Alt+F from the Fruit combo box",
                 [focused(role="menu"), said_any("File menu", "New")],
                 should="the File menu opens; the reader hears 'File menu' and 'New'",
                 tree=True):
        run.key("Alt+F")
    with run.act("Escape closes File",
                 [focused(role="menu item", name="File"), said("File")],
                 should="the menu closes and focus is on the File menu bar item"):
        run.key("Escape")
    with run.act("Escape again",
                 [focused(role="combo box")],
                 should="focus returns to the Fruit combo box the reader came from"):
        run.key("Escape")
    focus_combo(run, "fruit")
    with run.act("bare Alt tap from the Fruit combo box",
                 [focused(role="menu item", name="File"), said("File")],
                 should="focus moves to the File menu bar item and the reader says it"):
        run.key("Alt")
    with run.act("Alt+E on the menu bar (first open of Edit)",
                 [focused(role="menu"), said_any("Edit menu", "Undo")],
                 should="the Edit menu opens; the reader hears 'Edit menu' and 'Undo'"):
        run.key("Alt+E")
    log = printed(run)
    with run.act("bare C in the Edit menu (mnemonic of Copy)",
                 [log.check("Copy"), focused(role="combo box")],
                 should="Copy runs, the menu closes, and focus returns to the Fruit "
                 "combo box the reader entered the menu bar from"):
        run.key("c")
    log.done()


def escape_after_right(run):
    """Alt+F, Right to Edit, Escape: where does focus land?"""
    run.wait_for(role="combo box", nth=COMBO["fruit"])
    focus_combo(run, "fruit")
    with run.act("Alt+F from the Fruit combo box", [focused(role="menu")],
                 should="File opens"):
        run.key("Alt+F")
    with run.act("Right: the Edit menu",
                 [focused(role="menu"), said_any("Edit menu", "Undo")],
                 should="File closes, Edit opens; the reader hears 'Edit menu' and 'Undo'",
                 tree=True):
        run.key("Right")
    with run.act("Escape closes Edit",
                 [focused(role="menu item", name="Edit"), said("Edit")],
                 should="Edit closes and focus is on the Edit menu bar item, as after "
                 "Escape on a menu opened directly"):
        run.key("Escape")
    with run.act("Tab after that", [spoke_anything()],
                 should="Tab goes on from where the reader is, and the reader says where"):
        run.key("Tab")


def reopen(run):
    """The same menu opened three times."""
    run.wait_for(role="menu item", name="File")
    run.key("F10")
    run.wait(0.5)
    with run.act("Down opens File (first time)",
                 [focused(role="menu"), said("menu"), open_menu_live()],
                 should="the reader hears the menu open", tree=True):
        run.key("Down")
    with run.act("Escape", [focused(role="menu item", name="File"), said("File")],
                 should="back on File"):
        run.key("Escape")
    with run.act("Down opens File (second time)",
                 [focused(role="menu"), said("menu"), open_menu_live()],
                 should="the reader hears the menu open again, as the first time",
                 tree=True):
        run.key("Down")
    with run.act("Escape again", [focused(role="menu item", name="File"), said("File")],
                 should="back on File"):
        run.key("Escape")
    with run.act("Enter opens File (third time)",
                 [focused(role="menu"), said("menu"), open_menu_live()],
                 should="the reader hears the menu open", tree=True):
        run.key("Return")
    with run.act("Escape a third time", [said("File")], should="back on File"):
        run.key("Escape")


# ---------------------------------------------------------------------------
# Check and radio items (View menu)
# ---------------------------------------------------------------------------


def view_items(run):
    """The View menu opened once: what the tree says of its check, tri-state and
    radio items, and what the reader hears as the highlight moves over them."""
    run.wait_for(role="menu item", name="View")
    focus_combo(run, "fruit")
    radio = "radio menu item"
    with run.act("Alt+V opens View",
                 [focused(role="menu"), said_any("View menu", "Word Wrap"),
                  has_state("check menu item", "Word Wrap", "checked"),
                  has_state("check menu item", "Show Inspector", "indeterminate"),
                  has_state(radio, "Light Theme", "checked"),
                  lacks_state(radio, "Dark Theme", "checked"),
                  attr_is(radio, "Dark Theme", "posinset", "2"),
                  attr_is(radio, "Dark Theme", "setsize", "3"),
                  has_relation(radio, "Dark Theme", "member-of")],
                 should="the View menu opens; its items carry their states, and each "
                 "radio item its place in its group of three", tree=True):
        run.key("Alt+V")
    with run.act("Down: Word Wrap", [said("Word Wrap"), said("checked")],
                 should="the reader says 'Word Wrap check menu item checked'"):
        run.key("Down")
    with run.act("Down: Show Inspector (mixed)",
                 [said("Show Inspector"), said("partially checked")],
                 should="the reader says 'Show Inspector, partially checked'"):
        run.key("Down")
    with run.act("Down: Light Theme (radio 1 of 3)",
                 [said("Light Theme"), said("1 of 3")],
                 should="the reader says 'Light Theme radio menu item checked 1 of 3'"):
        run.key("Down")
    with run.act("Down: Dark Theme (radio 2 of 3)",
                 [said("Dark Theme"), said("2 of 3")],
                 should="the reader says 'Dark Theme radio menu item not checked 2 of 3'"):
        run.key("Down")
    with run.act("Enter: choose Dark Theme", [spoke_anything()],
                 should="Dark Theme is chosen, the menu closes, the reader hears where "
                 "focus lands"):
        run.key("Return")


def toggle_truth(run):
    """Turn Word Wrap off from the keyboard, reopen View, and read what the
    reader's tree says of Word Wrap against what the application holds."""
    run.wait_for(role="menu item", name="View")
    focus_combo(run, "fruit")
    with run.act("Alt+V (first open)",
                 [has_state("check menu item", "Word Wrap", "checked")],
                 should="Word Wrap is checked", tree=True):
        run.key("Alt+V")
    with run.act("Down, Space: Word Wrap off",
                 [focused(role="menu item", name="View")],
                 should="Word Wrap turns off and the menu closes"):
        run.key("Down")
        run.wait(0.4)
        run.key("space")
    with run.act("Alt+V (second open)",
                 [lacks_state("check menu item", "Word Wrap", "checked"),
                  open_menu_live(), said("menu")],
                 should="the reopened menu says Word Wrap is not checked", tree=True):
        run.key("Alt+V")
    with run.act("Escape", should="closed"):
        run.key("Escape")
    # The application's own truth, read after every act so nothing measured
    # above went through the bridge's own accessibility sync.
    try:
        bridge = run.bridge()
        run.note("the application holds Word Wrap toggled="
                 f"{_bridge_field(bridge, 'Word Wrap', 'toggled')} after the acts "
                 "(read through the bridge once the acts were over; the menu is closed, "
                 "so 'absent' means the node is not in the tree)")
        run.key("Alt+V")
        run.wait(1.0)
        run.note("with View open again, the application holds Word Wrap toggled="
                 f"{_bridge_field(bridge, 'Word Wrap', 'toggled')}")
    except Exception as exc:  # the note is a courtesy, not the measurement
        run.note(f"the bridge could not be read: {exc}")


def _bridge_field(bridge, label, field_name):
    snap = bridge.call("snapshot_tree")
    stack = [snap]
    while stack:
        cur = stack.pop()
        if isinstance(cur, dict):
            if cur.get("label") == label:
                return cur.get(field_name, "unset")
            stack.extend(cur.values())
        elif isinstance(cur, list):
            stack.extend(cur)
    return "absent"


# ---------------------------------------------------------------------------
# Submenu
# ---------------------------------------------------------------------------


def submenu(run):
    """File > Recent by keyboard, File opened once."""
    run.wait_for(role="menu item", name="File")
    focus_combo(run, "fruit")
    with run.act("Alt+F", [focused(role="menu")], should="File opens", tree=True):
        run.key("Alt+F")
    with run.act("Down x4: Recent (a submenu)",
                 [said("Recent")],
                 should="the reader says 'Recent', and that it opens a submenu"):
        run.key("Down", "Down", "Down", "Down")
    with run.act("Right opens Recent",
                 [said_any("project-alpha.toml", "Recent menu"), menus_open(2),
                  focused(role="menu")],
                 should="the submenu opens with focus in it; the reader hears it and its "
                 "first item; it is still open 2.5 s later", tree=True):
        run.key("Right")
    with run.act("Down in the submenu", [said("notes.md")],
                 should="the reader says 'notes.md'"):
        run.key("Down")
    with run.act("Left closes the submenu",
                 [said("Recent"), menus_open(1)],
                 should="the submenu closes and the reader is back on 'Recent'",
                 tree=True):
        run.key("Left")
    with run.act("Escape", [focused(role="menu item", name="File")],
                 should="File closes; focus on the File menu bar item"):
        run.key("Escape")


# ---------------------------------------------------------------------------
# Combo boxes
# ---------------------------------------------------------------------------


def combo_color(run):
    run.wait_for(role="combo box", nth=COMBO["color"])
    with run.act("focus the Color combo box (Blue selected)",
                 [said("Color"), said("Blue")],
                 should="the reader says 'Color combo box Blue'"):
        run.grab_focus(role="combo box", nth=COMBO["color"])
    with run.act("Down: Yellow",
                 [said("Yellow"), in_tree(role="list item", name="Yellow", state="selected"),
                  attr_is("list item", "Yellow", "posinset", "4"),
                  attr_is("list item", "Yellow", "setsize", "5")],
                 should="the list opens and the reader says the new value 'Yellow'; the "
                 "option carries 4 of 5", tree=True):
        run.key("Down")
    with run.act("Down: Purple", [said("Purple")], should="the reader says 'Purple'"):
        run.key("Down")
    with run.act("Enter closes the list",
                 [said("Purple"), not_in_tree(role="list box")],
                 should="the list closes on Purple (focus never left the combo box); the "
                 "reader hears the combo box's value 'Purple'", tree=True):
        run.key("Return")
    with run.act("Up (the list opens a second time): Yellow",
                 [said("Yellow")],
                 should="the list opens again and the reader says 'Yellow', as the first "
                 "time"):
        run.key("Up")
    with run.act("Escape", should="closed"):
        run.key("Escape")


def combo_fruit(run):
    run.wait_for(role="combo box", nth=COMBO["fruit"])
    with run.act("focus the Fruit combo box (nothing selected)",
                 [said("Fruit"), said("Select a fruit")],
                 should="the reader says 'Fruit combo box' and the placeholder "
                 "'Select a fruit...'"):
        run.grab_focus(role="combo box", nth=COMBO["fruit"])
    with run.act("Down from nothing selected",
                 [said("Apple"), in_tree(role="list item", name="Apple", state="selected")],
                 should="the list opens and the first fruit, Apple, is selected and "
                 "spoken", tree=True):
        run.key("Down")
    with run.act("Escape", [not_in_tree(role="list box")], should="the list closes",
                 tree=True):
        run.key("Escape")


def combo_huge(run):
    run.wait_for(role="combo box", nth=COMBO["huge"])
    with run.act("focus the Huge combo box", [said("Open me")],
                 should="the reader says the combo box and its placeholder"):
        run.grab_focus(role="combo box", nth=COMBO["huge"])
    with run.act("Alt+Down opens the 10 000-item list",
                 [in_tree(role="list box"),
                  count_of("list item", "the list exposes a window of options, not 10 000",
                           lambda n: 0 < n < 200),
                  said_any("10000", "10,000", "List")],
                 should="the list opens and the reader hears a list of 10 000 items",
                 tree=True):
        run.key("Alt+Down")
    with run.act("Down: the first item",
                 [said("Item #00000")],
                 should="the first item, Item #00000, is selected and spoken", tree=True):
        run.key("Down")
    with run.act("Down", [said("Item #00002")], should="the next item is spoken",
                 tree=True):
        run.key("Down")
    with run.act("End: the last item",
                 [said("Item #09999"),
                  in_tree(role="list item", name="Item #09999", state="selected"),
                  attr_is("list item", "Item #09999", "posinset", "10000"),
                  attr_is("list item", "Item #09999", "setsize", "10000")],
                 should="the last item is selected, in the tree, spoken, 10000 of 10000",
                 tree=True):
        run.key("End")
    with run.act("PageUp", [said("Item #09989"),
                            attr_is("list item", "Item #09989", "posinset", "9990")],
                 should="ten up: Item #09989 spoken, 9990 of 10000", tree=True):
        run.key("Page_Up")
    with run.act("Enter closes", [not_in_tree(role="list box"), said("Item #09989")],
                 should="the list closes on Item #09989 and the reader hears the value",
                 tree=True):
        run.key("Return")


def combo_reopen(run):
    """The Color list opened, closed and opened again."""
    run.wait_for(role="combo box", nth=COMBO["color"])
    focus_combo(run, "color")
    with run.act("Alt+Down (first open)", [said("List"), in_tree(role="list box")],
                 should="the list opens and the reader hears it", tree=True):
        run.key("Alt+Down")
    with run.act("Escape", should="closed"):
        run.key("Escape")
    with run.act("Alt+Down (second open)",
                 [said("List"),
                  node_facts("list box", "", "the list box is live (not defunct)",
                             lambda n: "defunct" not in n.get("states", []))],
                 should="the list opens again and the reader hears it, as the first time",
                 tree=True):
        run.key("Alt+Down")
    with run.act("Down: Yellow (in the reopened list)", [said("Yellow")],
                 should="the reader says 'Yellow'"):
        run.key("Down")
    with run.act("Escape", should="closed"):
        run.key("Escape")


def country_search(run):
    """The searchable combo box: focus moves into a search field."""
    run.wait_for(role="combo box", nth=COMBO["country"])
    focus_combo(run, "country")
    with run.act("Enter opens the searchable list",
                 [focused(role="entry"), spoke_anything()],
                 should="the list opens with focus in its search field; the reader "
                 "hears a named search field", tree=True):
        run.key("Return")
    with run.act("type 'fr'", [said("France")],
                 should="the list filters to France; the reader hears how many match "
                 "or the first match", tree=True):
        run.type("fr")
    with run.act("Down: the first match", [said("France")],
                 should="the reader hears 'France'", tree=True):
        run.key("Down")
    with run.act("Enter commits", [focused(role="combo box"), said("France")],
                 should="the list closes on France, focus is back on the combo box, "
                 "the reader hears its value", tree=True):
        run.key("Return")


# ---------------------------------------------------------------------------
# Context menus
# ---------------------------------------------------------------------------


def panel_reachable(title: str) -> Check:
    """The panel titled `title` (the one holding a label of that name), or a
    control inside it, is focusable or offers an action."""
    def run(act):
        for n in _walk(act.tree):
            if n.get("role") != "panel":
                continue
            kids = n.get("children", [])
            if not any(k.get("name") == title for k in kids):
                continue
            reach = [k for k in [n, *kids] if "focusable" in k.get("states", [])
                     or k.get("actions")]
            return bool(reach), [f"[panel] holding {title!r}: states={n.get('states')} "
                                 f"actions={n.get('actions')}; children: "
                                 + ", ".join(f"[{k.get('role')}] {k.get('name')!r} "
                                             f"{k.get('states')}" for k in kids)]
        return False, [f"no panel holding {title!r} in the tree"]
    return custom(f"the {title!r} panel (it owns a context menu) can be reached by "
                  "keyboard or offers an action", run, needs_tree=True)


def context_keys(run):
    """The two context-menu panels hold no focusable control: try every
    keyboard route a reader has, from the nearest control and on the panels."""
    run.wait_for(role="label", name="Edit Menu")
    with run.act("look at the panels",
                 [panel_reachable("Edit Menu"), panel_reachable("File Menu")],
                 should="a keyboard or screen reader user can reach the panel whose "
                 "context menu holds Cut/Copy/Paste", tree=True):
        pass
    focus_combo(run, "huge")
    with run.act("Shift+F10 on the Huge combo box (nearest focusable control)",
                 [in_tree(role="menu")],
                 should="some keyboard route opens the Edit or File context menu",
                 tree=True):
        run.key("Shift+F10")
    with run.act("Menu key", [in_tree(role="menu")],
                 should="some keyboard route opens the Edit or File context menu",
                 tree=True):
        run.key("Menu")
    with run.act("Tab from the Huge combo box",
                 [focused(role="panel")],
                 should="Tab reaches the Edit Menu panel, whose context menu a reader "
                 "could then open with Shift+F10"):
        run.key("Tab")


def context_mouse(run):
    """Open the Edit panel's context menu with the one route that exists (a
    secondary click, through the bridge: the harness has no pointer) and hear
    what the reader gets; then the File panel's."""
    run.wait_for(role="label", name="Edit Menu")
    focus_combo(run, "huge")
    bridge = run.bridge()
    node = _bridge_node_with_text(bridge, "Right-click for Cut/Copy/Paste")
    run.note(f"bridge node for the Edit panel's text: {node}")
    with run.act("right-click the Edit Menu panel (bridge)",
                 [focused(role="menu"), said("menu")],
                 should="the menu opens with focus in it; the reader hears 'menu' and "
                 "its first item", tree=True):
        run.note(f"right_click: {str(bridge.call('right_click', node=node))[:200]}")
    with run.act("Down", [said("Undo")], should="the reader says 'Undo'"):
        run.key("Down")
    with run.act("Escape: focus returns",
                 [focused(role="combo box"), said("combo box")],
                 should="the menu closes and focus returns to the Huge combo box, where "
                 "it was, which the reader says", tree=True):
        run.key("Escape")
    node = _bridge_node_with_text(bridge, "Right-click for file operations")
    with run.act("right-click the File Menu panel (bridge)",
                 [focused(role="menu"),
                  lacks_state("menu item", "Export as PDF", "enabled"),
                  lacks_state("menu item", "Export as PDF", "sensitive")],
                 should="the menu opens; 'Export as PDF', built disabled, is not "
                 "enabled to a reader", tree=True):
        run.note(f"right_click: {str(bridge.call('right_click', node=node))[:200]}")
    with run.act("Escape", should="closed"):
        run.key("Escape")


def _bridge_node_with_text(bridge, text):
    """The id of the first node in the bridge's snapshot whose label or value
    is `text` (a text label carries its words as a value)."""
    snap = bridge.call("snapshot_tree")
    stack = [snap]
    while stack:
        cur = stack.pop()
        if isinstance(cur, dict):
            if (cur.get("label") == text or cur.get("value") == text) and "id" in cur:
                ident = cur["id"]
                return ident if not isinstance(ident, dict) else next(iter(ident.values()))
            stack.extend(cur.values())
        elif isinstance(cur, list):
            stack.extend(cur)
    raise RuntimeError(f"no node carries {text!r} in the bridge snapshot: {str(snap)[:300]}")


# ---------------------------------------------------------------------------
# Popover menu and type-ahead
# ---------------------------------------------------------------------------


def popover_typeahead(run):
    run.wait_for(role="combo box", nth=COMBO["huge"])
    focus_combo(run, "huge")
    with run.act("Tab to Add", [focused(role="push button", name="Add"), said("Add")],
                 should="the reader says 'Add push button' (and that it opens a menu)"):
        run.key("Tab")
    with run.act("Enter opens its menu", [focused(role="menu"), said("New file")],
                 should="the menu opens and the reader hears 'New file'", tree=True):
        run.key("Return")
    with run.act("type-ahead 'n'", [said("New file")],
                 should="the highlight moves to the first item starting with n, "
                 "'New file', and the reader says it"):
        run.key("n")
    with run.act("type-ahead 'n' again (after the 500 ms reset)", [said("New folder")],
                 should="the highlight moves on to 'New folder' and the reader says it"):
        run.key("n")
    with run.act("type-ahead 'n' a third time", [said("New project")],
                 should="the highlight moves on to 'New project…'"):
        run.key("n")
    log = printed(run)
    with run.act("Enter runs New project", [log.check("NewProject"),
                                            focused(role="push button", name="Add")],
                 should="New project runs, the menu closes, focus is back on Add"):
        run.key("Return")
    log.done()


# ---------------------------------------------------------------------------
# What a screen reader's own activation does
# ---------------------------------------------------------------------------


def at_actions(run):
    run.wait_for(role="menu item", name="Edit")
    with run.act("AT-SPI click on the Edit menu bar item",
                 [focused(role="menu"), said("menu")],
                 should="the Edit menu opens with focus in it", tree=True):
        run.action("click", role="menu item", name="Edit")
    with run.act("AT-SPI grab_focus on 'Paste' in the open menu",
                 [focused(role="menu item", name="Paste"), said("Paste")],
                 should="a reader can put focus on an item of the open menu"):
        try:
            run.grab_focus(role="menu item", name="Paste")
        except Exception as exc:  # the refusal is the result
            run.note(f"grab_focus on 'Paste' in the open Edit menu: {exc}")
    log = printed(run)
    with run.act("AT-SPI click on 'Copy' in the open menu",
                 [log.check("Copy"), spoke_anything()],
                 should="Copy runs, the menu closes, the reader hears where focus lands"):
        run.action("click", role="menu item", name="Copy")
    log.done()
    with run.act("AT-SPI click on the File menu bar item",
                 [focused(role="menu")], should="File opens", tree=True):
        run.action("click", role="menu item", name="File")
    with run.act("AT-SPI click on 'Recent' (a submenu trigger)",
                 [menus_open(2), said_any("project-alpha.toml", "Recent")],
                 should="the Recent submenu opens and the reader hears it", tree=True):
        run.action("click", role="menu item", name="Recent")
    log2 = printed(run)
    with run.act("AT-SPI click on 'notes.md' in the submenu",
                 [log2.check("Recent: notes"), spoke_anything()],
                 should="the recent file opens, both menus close, the reader hears where "
                 "focus lands"):
        run.action("click", role="menu item", name="notes.md")
    log2.done()


# ---------------------------------------------------------------------------
# The rich-content menu
# ---------------------------------------------------------------------------


def rich_menu(run):
    """'View options' mixes buttons, a slider, a combo box and menu items in a
    MenuList. Can a keyboard / reader user reach them and use the slider?"""
    from reader_lib.checks import _focus_node, _is_focus

    run.wait_for(role="combo box", nth=COMBO["huge"])
    focus_combo(run, "huge")
    with run.act("Tab x3 to View options",
                 [focused(role="push button", name="View options"), said("View options")],
                 should="focus reaches the View options button"):
        run.key("Tab", "Tab", "Tab", gap=0.4)
    with run.act("Enter on View options",
                 [focused(role="menu"), in_tree(role="slider", name="Opacity")],
                 should="the menu opens; its slider, combo box and buttons are in the tree",
                 tree=True):
        run.key("Return")
    reached = False
    for i in range(14):
        with run.act(f"Tab {i + 1} inside the menu", [spoke_anything()],
                     should="focus moves to the next control of the menu and the reader "
                     "says it", settle=0.6, record=1.5) as act:
            run.key("Tab")
        run.collect_events(act)
        moves = [e for e in act.events if _is_focus(e)]
        node = _focus_node(moves[-1]) if moves else {}
        if node.get("role") == "slider":
            reached = True
            break
    run.note(f"Tab reached the Opacity slider inside the menu: {reached}")
    with run.act("Right on the slider (focus reached by Tab)",
                 [event("object:property-change:accessible-value", role="slider"),
                  said("0.66"),
                  node_facts("slider", "Opacity", "the reader's tree shows the slider moved",
                             lambda n: (n.get("value") or {}).get("current", 0) > 0.655)],
                 should="the value steps up by 0.01 and the reader hears the new value",
                 tree=True):
        run.key("Right")
    with run.act("End on the slider", [event("object:property-change:accessible-value",
                                             role="slider")],
                 should="the value jumps to 1.0 and the reader hears it", tree=True):
        run.key("End")
    with run.act("AT-SPI grab_focus on the Opacity slider",
                 [said("Opacity")],
                 should="a reader can put focus on the slider and hears its value"):
        run.grab_focus(role="slider", name="Opacity")
    with run.act("Right on the slider (focus put by AT-SPI)",
                 [event("object:property-change:accessible-value", role="slider"),
                  spoke_anything()],
                 should="the value changes and is spoken"):
        run.key("Right")
    # What the application itself holds, read once every act is over (the
    # bridge runs an accessibility sync of its own).
    try:
        bridge = run.bridge()
        run.note("with the menu still open, the application holds the Opacity slider at "
                 f"{_bridge_field(bridge, 'Opacity', 'numeric_value')!r} "
                 f"(node: {str(_bridge_node(bridge, 'Opacity'))[:300]})")
    except Exception as exc:
        run.note(f"the bridge could not be read: {exc}")


def _bridge_node(bridge, label):
    snap = bridge.call("snapshot_tree")
    stack = [snap]
    while stack:
        cur = stack.pop()
        if isinstance(cur, dict):
            if cur.get("label") == label and cur.get("role") in ("Slider", "slider"):
                return {k: v for k, v in cur.items() if k != "children"}
            stack.extend(cur.values())
        elif isinstance(cur, list):
            stack.extend(cur)
    return None


# ---------------------------------------------------------------------------
# Scrolled out and back
# ---------------------------------------------------------------------------


def scroll_back(run):
    """Tab from the Huge combo box to Add scrolls the page: the combo box
    section leaves the viewport, and AccessKit's filter drops it from the
    reader's tree. Shift+Tab brings it back, under the same node ids."""
    run.wait_for(role="combo box", nth=COMBO["huge"])
    with run.act("look at the tree at launch",
                 [lacks_state("menu item", "Disabled item", "enabled"),
                  lacks_state("menu item", "Disabled item", "sensitive"),
                  in_tree(role="push button", name="View options")],
                 should="'Disabled item' (built with enabled(false)) is disabled to a "
                 "reader; the whole window's controls are in the tree", tree=True):
        pass
    focus_combo(run, "huge")
    with run.act("Tab to Add (the page scrolls)",
                 [focused(role="push button", name="Add"), said("Add")],
                 should="focus moves to Add and the reader says it"):
        run.key("Tab")
    with run.act("Tab to Search (the combo boxes leave the view)",
                 [focused(role="push button", name="Search"), said("Search")],
                 should="focus moves to Search; the page scrolls on", tree=True):
        run.key("Tab")
    with run.act("Shift+Tab back to Add", [focused(role="push button", name="Add"),
                                           said("Add")],
                 should="the reader says 'Add'"):
        run.key("Shift+Tab")
    with run.act("Shift+Tab back to the Huge combo box (scrolled back into view)",
                 [focused(role="combo box"), said("combo box"),
                  node_facts("combo box", "", "the combo boxes are live (not defunct)",
                             lambda n: "defunct" not in n.get("states", []))],
                 should="focus returns to the Huge combo box and the reader says it",
                 tree=True):
        run.key("Shift+Tab")
    with run.act("Shift+Tab to the Country combo box",
                 [focused(role="combo box"), said("combo box")],
                 should="the reader says the Country combo box"):
        run.key("Shift+Tab")


def submenu_enter(run):
    """File > Recent opened with Enter instead of Right; File opened once."""
    run.wait_for(role="menu item", name="File")
    focus_combo(run, "fruit")
    with run.act("Alt+F, Down x4 (Recent highlighted)", [focused(role="menu")],
                 should="File opens"):
        run.key("Alt+F")
        run.wait(0.4)
        run.key("Down", "Down", "Down", "Down")
    with run.act("Enter opens Recent; still open 2.5 s later",
                 [menus_open(2), focused(role="menu"),
                  no_event("object:children-changed:remove", role="frame")],
                 should="the submenu opens with focus in it and stays open", tree=True):
        run.key("Return")
    with run.act("Enter on the submenu's first item",
                 [spoke_anything()],
                 should="if the submenu is still open, project-alpha.toml opens; the "
                 "reader hears where focus lands"):
        run.key("Return")


SCENARIOS = [
    Scenario("menus-menubar-f10", PKG, menubar_f10,
             "F10, open File with Down, arrow through it, Escape twice"),
    Scenario("menus-f10-escape", PKG, f10_escape,
             "F10 from a combo box, Escape: does focus come back?"),
    Scenario("menus-menubar-alt", PKG, menubar_alt,
             "Alt+F from content, Escape twice, bare Alt, Alt+E, an in-menu mnemonic"),
    Scenario("menus-escape-after-right", PKG, escape_after_right,
             "Alt+F, Right to Edit, Escape: where focus lands"),
    Scenario("menus-reopen", PKG, reopen, "the File menu opened three times"),
    Scenario("menus-view-items", PKG, view_items,
             "View menu: check, tri-state and radio items, their states and positions"),
    Scenario("menus-toggle-truth", PKG, toggle_truth,
             "Word Wrap toggled; the reopened menu against the application's state"),
    Scenario("menus-submenu", PKG, submenu, "File > Recent submenu by keyboard"),
    Scenario("menus-submenu-enter", PKG, submenu_enter,
             "File > Recent opened with Enter: does it stay open?"),
    Scenario("menus-combo-color", PKG, combo_color,
             "the Color combo box: its value, arrows, Enter, a second opening"),
    Scenario("menus-combo-fruit", PKG, combo_fruit,
             "the Fruit combo box: placeholder, first Down"),
    Scenario("menus-combo-huge", PKG, combo_huge,
             "the 10 000-item combo box: tree size, arrows, End, PageUp, positions"),
    Scenario("menus-combo-reopen", PKG, combo_reopen,
             "the Color list opened twice"),
    Scenario("menus-country-search", PKG, country_search,
             "the searchable Country combo box: open, filter, arrow, commit"),
    Scenario("menus-context-keys", PKG, context_keys,
             "keyboard routes to the two context menus"),
    Scenario("menus-context-mouse", PKG, context_mouse,
             "the Edit and File panels' context menus opened by a secondary click"),
    Scenario("menus-popover-typeahead", PKG, popover_typeahead,
             "the Add popover menu: open, type-ahead, Enter"),
    Scenario("menus-at-actions", PKG, at_actions,
             "AT-SPI click and grab_focus on menu bar items and on items of open menus"),
    Scenario("menus-rich-menu", PKG, rich_menu,
             "View options: a slider, a combo box and buttons inside a MenuList"),
    Scenario("menus-scroll-back", PKG, scroll_back,
             "controls scrolled out of view and back; disabled menu item state"),
]
