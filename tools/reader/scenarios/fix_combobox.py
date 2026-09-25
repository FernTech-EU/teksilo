# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""The ComboBox popup as a screen reader meets it, for the fix of the sweep's
combobox findings (misc-05, menus-07, menus-14, catalog-a-09, winintl-04,
misc-22, menus-18).

Where each piece comes from (`crates/teksilo-widgets/src/combo_box.rs`,
`combo_box/panel.rs`, `combo_box/item.rs`):

* The keys move a highlight through the open list and only Enter commits it,
  the ARIA select-only combobox pattern. Down from an empty combo box reaches
  the first option. Escape closes and keeps the value.
* The node that holds focus (the combo box, or a searchable combo's search
  field) names the highlighted option as its `active_descendant`, which
  `accesskit_consumer` resolves as the focus: each option reached raises
  `state-changed:focused` on it.
* The open list is one list box of named options, straight under the combo
  box: a long list's `ListView` publishes nothing of its own, the combo box
  `controls` the list box itself (no `[unknown]` host between them), and the
  highlighted option is the one selected child AT-SPI's Selection interface
  reports.
* The search field is named after the combo box (or "Search").
"""

from __future__ import annotations

from reader_lib.checks import (_focus_node, _is_focus, _walk, custom, focused, in_tree,
                               no_event, not_in_tree, said)
from reader_lib.orca import normalized, utterances
from reader_lib.scenario import Scenario
from scenarios.menus import COMBO, attr_is, focus_combo
from scenarios.misc import scene
from scenarios.windows_i18n import FR, names_in_tree

MENUS = "menus-and-dropdowns"


def one_flat_list_box() -> object:
    """One list box in the tree, holding named options that wrap no other
    option, with exactly one selected child on AT-SPI's Selection interface
    (what Orca reads on the list box's `selection-changed`)."""
    def run(act):
        boxes = [n for n in _walk(act.tree) if n.get("role") == "list box"]
        if len(boxes) != 1:
            return False, [f"{len(boxes)} list boxes in the tree: "
                           + ", ".join(f"{b.get('name')!r} {b.get('states')}" for b in boxes)]
        box = boxes[0]
        items = [n for n in _walk(box) if n.get("role") == "list item"]
        unnamed = [n for n in items if not n.get("name")]
        wrapping = [n for n in items
                    if any(c.get("role") == "list item" for c in list(_walk(n))[1:])]
        selected = box.get("selected_children")
        ok = bool(items) and not unnamed and not wrapping and selected == 1
        return ok, [f"{len(items)} list items, {len(unnamed)} unnamed, {len(wrapping)} "
                    f"wrapping another; the list box reports {selected} selected children",
                    "selected: " + ", ".join(repr(n.get("name")) for n in items
                                             if "selected" in n.get("states", []))]
    return custom("one list box of named options, one of them selected", run, needs_tree=True)


def focus_said() -> object:
    """Orca said, uncut, the name of the node the act's last focus change
    landed on (for lists whose content varies by machine: the fonts)."""
    def run(act):
        moves = [e for e in act.events if _is_focus(e)]
        if not moves:
            return False, ["no focus change on the bus in this act"]
        name = _focus_node(moves[-1]).get("name") or ""
        heard = utterances(act.orca)
        ok = bool(name) and any(not u.cut and normalized(name) in normalized(u.text)
                                for u in heard)
        return ok, [f"last focus: {name!r}"] + [f"Orca said: {u.text!r}" for u in heard]
    return custom("Orca says the name of the node focus lands on", run, needs_orca=True)


def option(name: str) -> list:
    """The reader's focus lands on the option `name` and Orca says it."""
    return [focused(role="list item", name=name), said(name)]


# ---------------------------------------------------------------------------
# menus-and-dropdowns
# ---------------------------------------------------------------------------


def fruit(run):
    run.wait_for(role="combo box", nth=COMBO["fruit"])
    focus_combo(run, "fruit")
    with run.act("Down from nothing selected",
                 [*option("Apple"), one_flat_list_box(), not_in_tree(role="unknown")],
                 should="the list opens on the first fruit, Apple, and the reader hears it",
                 tree=True):
        run.key("Down")
    with run.act("Down", option("Banana"), should="the reader hears the next fruit"):
        run.key("Down")
    with run.act("Escape keeps nothing",
                 [focused(role="combo box"), not_in_tree(role="list box")],
                 should="the list closes and focus is back on the combo box", tree=True):
        run.key("Escape")
    # The list opens a second time from here on. Orca drops everything in a
    # reopened list as defunct (it comes back under the ids the adapter
    # retired: the node-ids topic, menus-04), so these acts check the bus.
    with run.act("Down again reaches Apple: Banana was never committed",
                 [focused(role="list item", name="Apple")],
                 should="the combo box is still empty, so Down reaches the first fruit "
                 "(Orca's speech on a reopened list is the node-ids topic)"):
        run.key("Down")
    with run.act("Enter commits Apple",
                 [focused(role="combo box"), not_in_tree(role="list box")],
                 should="the list closes on Apple and focus is back on the combo box",
                 tree=True):
        run.key("Return")
    with run.act("Alt+Down opens on the value", [focused(role="list item", name="Apple")],
                 should="the list opens on Apple, the value (Orca's speech on a reopened "
                 "list is the node-ids topic)"):
        run.key("Alt+Down")
    with run.act("Escape", should="closed"):
        run.key("Escape")


def huge(run):
    run.wait_for(role="combo box", nth=COMBO["huge"])
    focus_combo(run, "huge")
    with run.act("Alt+Down opens the 10 000-item list on its first item",
                 [*option("Item #00000"), one_flat_list_box(), not_in_tree(role="unknown"),
                  attr_is("list item", "Item #00000", "setsize", "10000")],
                 should="the list opens on the first item; the reader hears it, 1 of 10000",
                 tree=True):
        run.key("Alt+Down")
    with run.act("Down", option("Item #00001"), should="the reader hears the next item"):
        run.key("Down")
    with run.act("End: the last item",
                 [*option("Item #09999"), one_flat_list_box(),
                  attr_is("list item", "Item #09999", "posinset", "10000")],
                 should="the reader hears the last item, 10000 of 10000", tree=True):
        run.key("End")
    with run.act("PageUp", option("Item #09989"),
                 should="ten items up: the reader hears Item #09989"):
        run.key("Page_Up")
    with run.act("Enter commits it",
                 [focused(role="combo box"), not_in_tree(role="list box")],
                 should="the list closes on Item #09989, focus back on the combo box",
                 tree=True):
        run.key("Return")


def country(run):
    run.wait_for(role="combo box", nth=COMBO["country"])
    focus_combo(run, "country")
    with run.act("Enter opens the searchable list",
                 [focused(role="entry", name="Search"), said("Search"),
                  not_in_tree(role="unknown")],
                 should="focus moves into a named search field (the combo box has no "
                 "label of its own, so it is 'Search')", tree=True):
        run.key("Return")
    with run.act("type 'fr'", [in_tree(role="list item", name="France")],
                 should="the list filters to France; focus stays in the search field",
                 tree=True):
        run.type("fr")
    with run.act("Down: the first match", [*option("France"), one_flat_list_box()],
                 should="the reader hears France", tree=True):
        run.key("Down")
    with run.act("Enter commits France",
                 [focused(role="combo box"), not_in_tree(role="list box")],
                 should="the list closes on France, focus back on the combo box", tree=True):
        run.key("Return")


# ---------------------------------------------------------------------------
# font-picker
# ---------------------------------------------------------------------------


def font(run):
    with scene(run, "Tab to Font family"):
        run.key("Tab", "Tab")
    with run.act("Alt+Down opens the font list",
                 [focused(role="entry", name="Font family"), said("Font family"),
                  not_in_tree(role="unknown")],
                 should="focus moves into the list's search field, named after the picker",
                 tree=True, record=3.0):
        run.key("Alt+Down")
    with run.act("Down: the first font",
                 [focused(role="list item"), focus_said(), one_flat_list_box()],
                 should="the reader hears the first font's family name", tree=True):
        run.key("Down")
    with run.act("Down: the next font", [focused(role="list item"), focus_said()],
                 should="the reader hears the next font's family name"):
        run.key("Down")
    with run.act("type 'mono' to search",
                 [focused(role="entry", name="Font family")],
                 should="typing puts the reader back in the search field", tree=True,
                 record=3.0):
        run.type("mono")
    with run.act("Down: the first monospaced font",
                 [focused(role="list item", name_contains="Mono"), focus_said()],
                 should="the reader hears the first match"):
        run.key("Down")
    with run.act("Enter picks it", [focused(role="combo box", name="Font family")],
                 should="the list closes and focus is back on the picker", record=3.0):
        run.key("Return")
    with scene(run, "Tab twice to Writing system"):
        run.key("Tab", "Tab")
    with run.act("Alt+Down opens the writing-system list",
                 [focused(role="list item"), focus_said(), one_flat_list_box()],
                 should="the list opens and the reader hears the current script",
                 tree=True, record=3.0):
        run.key("Alt+Down")
    with run.act("Down in the writing-system list", option("Latin"),
                 should="the reader hears the next script, Latin"):
        run.key("Down")
    with run.act("Enter picks it", [focused(role="combo box", name="Writing system")],
                 should="the list closes and focus is back on the combo box", record=3.0):
        run.key("Return")


# ---------------------------------------------------------------------------
# internationalization
# ---------------------------------------------------------------------------


def language(run):
    run.wait_for(role="combo box", name="Language")
    run.grab_focus(role="push button", name="العربية")
    with scene(run, "Tab to the language combo box"):
        run.key("Tab")
    with run.act("Down to the next language",
                 [focused(role="list item", name_contains="fr-FR"), said("fr-FR"),
                  no_event("object:property-change:accessible-name", role="label",
                           name_contains="Bonjour"),
                  names_in_tree("Hello, Alice!")],
                 should="the reader hears français; the application stays in English",
                 tree=True, record=3.0):
        run.key("Down")
    with run.act("Escape keeps English",
                 [names_in_tree("Hello, Alice!"), focused(role="combo box", name="Language")],
                 should="the list closes and nothing changed", tree=True, record=3.0):
        run.key("Escape")
    with run.act("Down, Enter switches to French", [names_in_tree(FR["heading"])],
                 should="the application switches to French only on Enter", tree=True,
                 record=3.0):
        run.key("Down")
        run.key("Return")


SCENARIOS = [
    Scenario("fix-combobox-fruit", MENUS, fruit,
             "the Fruit combo box: Down from empty, Escape keeps nothing, Enter commits"),
    Scenario("fix-combobox-huge", MENUS, huge,
             "the 10 000-item combo box: every move spoken, one flat list box"),
    Scenario("fix-combobox-country", MENUS, country,
             "the searchable Country combo box: a named search field, matches spoken"),
    Scenario("fix-combobox-font", "font-picker", font,
             "the font list (searchable) and the writing-system list (not)"),
    Scenario("fix-combobox-language", "internationalization", language,
             "the LanguageSwitcher: arrowing switches nothing, Enter does"),
]
