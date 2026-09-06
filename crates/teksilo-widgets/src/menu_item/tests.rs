// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tests for [`MenuItem`].
use super::*;
use crate::menu_list::MenuList;
use teksilo_core::accesskit::Role;
use teksilo_core::event::Modifiers;
use teksilo_core::widget_tree::WidgetTree;

fn tree() -> WidgetTree {
    WidgetTree::new().with_theme(teksilo_core::presets::intui::light())
}

fn layout(tree: &mut WidgetTree) {
    tree.layout(SizeProposal::exact(400.0, 300.0));
}

// --- `MenuItemStyle::highlighted_label_role` ---

/// A style that fills a highlighted row with a saturated colour has to
/// be able to recolour the label on top of it, and it cannot do that
/// from `make_body` — `MenuItem` builds its label first.
#[derive(Debug, Default, Clone, Copy)]
struct OnAccentHighlightStyle;

impl teksilo_core::styles::MenuItemStyle for OnAccentHighlightStyle {
    fn make_body(
        &self,
        cfg: &MenuItemStyleConfig,
        ctx: &mut teksilo_core::build_context::BuildContext,
    ) -> WidgetId {
        crate::styles::RecipeMenuItemStyle::for_tokens(&ctx.theme().input).make_body(cfg, ctx)
    }

    fn highlighted_label_role(&self) -> Option<TextRole> {
        Some(TextRole::OnAccent)
    }
}

/// A theme whose `text_on_accent` differs from `text_primary`. IntUI's
/// are both black — it pairs black labels with its teal accent — so
/// the stock preset cannot tell a flipped label from an unflipped one.
fn discriminating_theme() -> teksilo_core::Theme {
    let mut t = teksilo_core::presets::intui::light();
    t.colors.text_on_accent = teksilo_tokens::Color::WHITE;
    assert_ne!(t.colors.text_primary, t.colors.text_on_accent);
    t
}

fn glyph_colors(tree: &mut WidgetTree) -> Vec<[u8; 4]> {
    tree.render()
        .glyphs
        .iter()
        .map(|g| {
            let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            [q(g.color[0]), q(g.color[1]), q(g.color[2]), q(g.color[3])]
        })
        .collect()
}

fn rgba8(c: teksilo_tokens::Color) -> [u8; 4] {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [q(c.r()), q(c.g()), q(c.b()), q(c.a())]
}

/// Build a menu row under `theme`, optionally hover it with a real
/// pointer move, and report the glyph colours it paints.
fn row_glyph_colors(theme: teksilo_core::Theme, hovered: bool, styled: bool) -> Vec<[u8; 4]> {
    let mut t = WidgetTree::new()
        .with_theme(theme)
        .with_text_backend(std::rc::Rc::new(std::cell::RefCell::new(
            teksilo_canvas::MockTextBackend::new(),
        )));
    let mut item = MenuItem::new(lit!("Open"));
    if styled {
        item = item.style(OnAccentHighlightStyle);
    }
    let id = t.add(item);
    layout(&mut t);
    if hovered {
        // A real pointer move rather than poking the interaction
        // signal: it exercises the same path the running app takes,
        // and the signal is private to `build`.
        t.pointer_move(t.bounds(id).center());
        layout(&mut t);
    }
    glyph_colors(&mut t)
}

/// The default is `None`, and a row under it keeps its own mapping
/// however it is highlighted — the behaviour IntUI and Fluent rely on.
#[test]
fn a_style_without_the_hook_leaves_the_highlighted_label_alone() {
    let theme = discriminating_theme();
    let primary = rgba8(theme.colors.text_primary);
    let on_accent = rgba8(theme.colors.text_on_accent);

    let colors = row_glyph_colors(theme, true, false);
    assert!(colors.contains(&primary));
    assert!(!colors.contains(&on_accent));
}

/// …and a style that declares it flips the label while highlighted.
#[test]
fn the_hook_flips_the_label_of_a_highlighted_row() {
    let theme = discriminating_theme();
    let on_accent = rgba8(theme.colors.text_on_accent);
    assert!(row_glyph_colors(theme, true, true).contains(&on_accent));
}

/// An idle row must keep its normal label even under a style that
/// declares the hook, or every row in the menu would read as chosen.
#[test]
fn the_hook_does_not_touch_an_idle_row() {
    let theme = discriminating_theme();
    let primary = rgba8(theme.colors.text_primary);
    let on_accent = rgba8(theme.colors.text_on_accent);

    let colors = row_glyph_colors(theme, false, true);
    assert!(colors.contains(&primary));
    assert!(!colors.contains(&on_accent));
}

// --- Role coverage ---

fn a11y_node(
    update: &teksilo_core::accesskit::TreeUpdate,
    id: teksilo_core::widget_id::WidgetId,
) -> &teksilo_core::accesskit::Node {
    let nid = teksilo_core::accessibility::widget_id_to_node_id(id);
    update
        .nodes
        .iter()
        .find(|(node_id, _)| *node_id == nid)
        .map(|(_, n)| n)
        .expect("widget present in the accessibility tree")
}

// --- Trailing hint (descriptive phrase, not an accelerator) ---

/// The whole point of `trailing_hint` over `shortcut_label`: a phrase like
/// "inside" must reach AT as a *description*. Routed through
/// `keyboard_shortcut` (as `shortcut_label` does) a screen reader would
/// announce it as a chord the user should press.
#[test]
fn trailing_hint_is_announced_as_a_description_not_a_chord() {
    let mut t = tree();
    let list_id =
        t.add(MenuList::new().item(MenuItem::new(lit!("Scene")).trailing_hint(lit!("inside"))));
    layout(&mut t);
    let item_id = first_descendant_with_role(&t, list_id, Role::MenuItem);
    let update = t.sync_accessibility();
    let node = a11y_node(&update, item_id);
    assert_eq!(node.description(), Some("inside"));
    assert_eq!(
        node.keyboard_shortcut(),
        None,
        "a descriptive hint must never be announced as a keyboard shortcut"
    );
}

/// The sibling guarantee — `shortcut_label` keeps its accelerator
/// semantics, and does not leak into the description slot.
#[test]
fn shortcut_label_stays_a_chord_and_sets_no_description() {
    let mut t = tree();
    let list_id = t.add(MenuList::new().item(MenuItem::new(lit!("Save")).shortcut_label("Ctrl+S")));
    layout(&mut t);
    let item_id = first_descendant_with_role(&t, list_id, Role::MenuItem);
    let update = t.sync_accessibility();
    let node = a11y_node(&update, item_id);
    assert_eq!(node.keyboard_shortcut(), Some("Ctrl+S"));
    assert_eq!(node.description(), None);
}

/// Both may coexist: the chord and the phrase occupy the same trailing
/// row but neither displaces the other, in the render or in AT.
#[test]
fn a_chord_and_a_hint_coexist_without_displacing_each_other() {
    let mut t = tree();
    let list_id = t.add(
        MenuList::new().item(
            MenuItem::new(lit!("Duplicate"))
                .shortcut_label("Ctrl+D")
                .trailing_hint(lit!("after")),
        ),
    );
    layout(&mut t);
    let item_id = first_descendant_with_role(&t, list_id, Role::MenuItem);
    let update = t.sync_accessibility();
    let node = a11y_node(&update, item_id);
    assert_eq!(node.keyboard_shortcut(), Some("Ctrl+D"));
    assert_eq!(node.description(), Some("after"));
}

#[test]
fn plain_item_emits_role_menuitem() {
    let mut t = tree();
    let list_id = t.add(MenuList::new().item(MenuItem::new(lit!("Save"))));
    layout(&mut t);
    let item_id = first_descendant_with_role(&t, list_id, Role::MenuItem);
    let info = t.accessibility_node(item_id);
    assert_eq!(info.role(), Role::MenuItem);
    assert_eq!(info.name(), Some("Save"));
}

#[test]
fn checked_emits_role_menuitemcheckbox() {
    let checked = Signal::new(false);
    let mut t = tree();
    let list_id = t.add(MenuList::new().item(MenuItem::new(lit!("Word Wrap")).checked(checked)));
    layout(&mut t);
    let item_id = first_descendant_with_role(&t, list_id, Role::MenuItemCheckBox);
    let info = t.accessibility_node(item_id);
    assert_eq!(info.role(), Role::MenuItemCheckBox);
    assert_eq!(info.name(), Some("Word Wrap"));
    assert!(!info.is_toggled());
}

#[test]
fn check_state_emits_role_menuitemcheckbox() {
    let state = Signal::new(CheckState::Unchecked);
    let mut t = tree();
    let list_id =
        t.add(MenuList::new().item(MenuItem::new(lit!("Show Inspector")).check_state(state)));
    layout(&mut t);
    let item_id = first_descendant_with_role(&t, list_id, Role::MenuItemCheckBox);
    let info = t.accessibility_node(item_id);
    assert_eq!(info.role(), Role::MenuItemCheckBox);
}

#[test]
fn radio_emits_role_menuitemradio() {
    let sel = Signal::new(0_usize);
    let mut t = tree();
    let list_id = t.add(MenuList::new().item(MenuItem::new(lit!("Light")).radio(0, sel.clone())));
    layout(&mut t);
    let item_id = first_descendant_with_role(&t, list_id, Role::MenuItemRadio);
    let info = t.accessibility_node(item_id);
    assert_eq!(info.role(), Role::MenuItemRadio);
    assert!(info.is_toggled());
}

// --- Activation: state mutation ---

#[test]
fn checked_click_flips_signal() {
    let checked = Signal::new(false);
    let mut t = tree();
    let list_id =
        t.add(MenuList::new().item(MenuItem::new(lit!("Word Wrap")).checked(checked.clone())));
    layout(&mut t);
    let item_id = first_descendant_with_role(&t, list_id, Role::MenuItemCheckBox);
    t.click(item_id);
    assert!(checked.get());
    // Re-add and click again to confirm round-trip — but the menu
    // already dismissed; rebuild a fresh tree to test the second flip.
    let mut t2 = tree();
    let checked2 = Signal::new(true);
    let list_id2 =
        t2.add(MenuList::new().item(MenuItem::new(lit!("Word Wrap")).checked(checked2.clone())));
    layout(&mut t2);
    let item_id2 = first_descendant_with_role(&t2, list_id2, Role::MenuItemCheckBox);
    t2.click(item_id2);
    assert!(!checked2.get());
}

#[test]
fn reflect_checked_emits_role_and_reflects_signal() {
    let visible = Signal::new(true);
    let mut t = tree();
    let list_id =
        t.add(MenuList::new().item(MenuItem::new(lit!("Show Outline")).reflect_checked(visible)));
    layout(&mut t);
    let item_id = first_descendant_with_role(&t, list_id, Role::MenuItemCheckBox);
    let info = t.accessibility_node(item_id);
    assert_eq!(info.role(), Role::MenuItemCheckBox);
    assert!(
        info.is_toggled(),
        "checkmark reflects the bound signal (true)"
    );
}

#[test]
fn reflect_checked_click_does_not_write_signal() {
    // The defining property: activation is reflect-only — the bound signal's
    // truth lives elsewhere, so clicking must NOT flip it (the on_activate /
    // intent owns the change).
    let visible = Signal::new(false);
    let mut t = tree();
    let list_id = t.add(
        MenuList::new().item(MenuItem::new(lit!("Show Outline")).reflect_checked(visible.clone())),
    );
    layout(&mut t);
    let item_id = first_descendant_with_role(&t, list_id, Role::MenuItemCheckBox);
    t.click(item_id);
    assert!(
        !visible.get(),
        "reflect_checked must not write the bound signal on click"
    );
}

#[test]
fn check_state_click_cycles_two_states_not_three() {
    // Mirror Checkbox: click toggles Unchecked <-> Checked only.
    // Indeterminate (external) promotes to Checked on click.
    let state = Signal::new(CheckState::Unchecked);
    let mut t = tree();
    let list_id =
        t.add(MenuList::new().item(MenuItem::new(lit!("Inspector")).check_state(state.clone())));
    layout(&mut t);
    let item_id = first_descendant_with_role(&t, list_id, Role::MenuItemCheckBox);
    t.click(item_id);
    assert_eq!(state.get(), CheckState::Checked);

    let state2 = Signal::new(CheckState::Checked);
    let mut t2 = tree();
    let list_id2 =
        t2.add(MenuList::new().item(MenuItem::new(lit!("Inspector")).check_state(state2.clone())));
    layout(&mut t2);
    let item_id2 = first_descendant_with_role(&t2, list_id2, Role::MenuItemCheckBox);
    t2.click(item_id2);
    assert_eq!(state2.get(), CheckState::Unchecked);

    let state3 = Signal::new(CheckState::Indeterminate);
    let mut t3 = tree();
    let list_id3 =
        t3.add(MenuList::new().item(MenuItem::new(lit!("Inspector")).check_state(state3.clone())));
    layout(&mut t3);
    let item_id3 = first_descendant_with_role(&t3, list_id3, Role::MenuItemCheckBox);
    t3.click(item_id3);
    // Indeterminate -> Checked (promotion, not cycle to Unchecked).
    assert_eq!(state3.get(), CheckState::Checked);
}

#[test]
fn radio_click_writes_value_to_shared_signal() {
    let sel = Signal::new(0_usize);
    let mut t = tree();
    let _list_id = t.add(
        MenuList::new()
            .item(MenuItem::new(lit!("Light")).radio(0, sel.clone()))
            .item(MenuItem::new(lit!("Dark")).radio(1, sel.clone()))
            .item(MenuItem::new(lit!("System")).radio(2, sel.clone())),
    );
    layout(&mut t);
    // Find the "Dark" item by label.
    let dark_id = t
        .find_by_label("Dark")
        .expect("Dark menu item should exist");
    t.click(dark_id);
    assert_eq!(sel.get(), 1);
}

#[test]
fn checked_space_keypress_flips_signal() {
    let checked = Signal::new(false);
    let mut t = tree();
    let list_id =
        t.add(MenuList::new().item(MenuItem::new(lit!("Word Wrap")).checked(checked.clone())));
    layout(&mut t);
    let item_id = first_descendant_with_role(&t, list_id, Role::MenuItemCheckBox);
    t.focus(item_id);
    t.press_key(Key::Space, Modifiers::NONE);
    assert!(checked.get());
}

#[test]
fn radio_external_signal_change_reflects_in_at() {
    // The bound `Signal<usize>` is the source of truth; clicking is
    // only one path. An external write must flip every item's
    // is_toggled() the next time the AT walker reads it.
    let sel = Signal::new(0_usize);
    let mut t = tree();
    let list_id = t.add(
        MenuList::new()
            .item(MenuItem::new(lit!("Light")).radio(0, sel.clone()))
            .item(MenuItem::new(lit!("Dark")).radio(1, sel.clone())),
    );
    layout(&mut t);
    let light_id = t.find_by_label("Light").expect("Light exists");
    let dark_id = t.find_by_label("Dark").expect("Dark exists");

    assert!(t.accessibility_node(light_id).is_toggled());
    assert!(!t.accessibility_node(dark_id).is_toggled());

    sel.set(1);
    let _ = list_id;
    assert!(!t.accessibility_node(light_id).is_toggled());
    assert!(t.accessibility_node(dark_id).is_toggled());
}

// --- Reactive role state ---

#[test]
fn checked_at_state_reflects_signal() {
    let checked = Signal::new(true);
    let mut t = tree();
    let list_id =
        t.add(MenuList::new().item(MenuItem::new(lit!("Word Wrap")).checked(checked.clone())));
    layout(&mut t);
    let item_id = first_descendant_with_role(&t, list_id, Role::MenuItemCheckBox);
    assert!(t.accessibility_node(item_id).is_toggled());
    checked.set(false);
    assert!(!t.accessibility_node(item_id).is_toggled());
}

// --- Mnemonic plumbing ---

#[test]
fn ampersand_stripped_from_at_name() {
    // The `&` marker is parsed out of the label so screen readers
    // don't announce "ampersand Save" — they announce "Save".
    let mut t = tree();
    let list_id = t.add(MenuList::new().item(MenuItem::new(lit!("&Save"))));
    layout(&mut t);
    let item_id = first_descendant_with_role(&t, list_id, Role::MenuItem);
    let info = t.accessibility_node(item_id);
    assert_eq!(info.name(), Some("Save"));
}

#[test]
fn mnemonic_parsed_from_label_when_builder_returns() {
    // Build the item, drop it back to inspect — the mnemonic
    // accessor should reflect the parse.
    let mut mi = MenuItem::new(lit!("&File"));
    mi.ensure_mnemonic_parsed();
    let m = mi.mnemonic().expect("mnemonic exists");
    assert_eq!(m.stripped, "File");
    assert_eq!(m.key_lower, Some('f'));
}

// --- Plain item AT smoke ---

// --- Helpers ---

fn first_descendant_with_role(t: &WidgetTree, from: WidgetId, role: Role) -> WidgetId {
    // BFS through the tree starting at `from`.
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(from);
    while let Some(id) = queue.pop_front() {
        if t.accessibility_node(id).role() == role {
            return id;
        }
        for child in t.children(id) {
            queue.push_back(child);
        }
    }
    panic!("no descendant of {from:?} has role {role:?}");
}

// --- Regression: shortcut-registry churn must not rebuild a
//     shortcut-bearing menu item (which would drop its click) ---

/// Every widget id in the subtree rooted at `from`, breadth-first.
fn subtree(t: &WidgetTree, from: WidgetId) -> Vec<WidgetId> {
    let mut out = Vec::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(from);
    while let Some(id) = queue.pop_front() {
        out.push(id);
        for child in t.children(id) {
            queue.push_back(child);
        }
    }
    out
}

/// Regression: a signal-bound `.enabled(...)` that starts `false` and
/// later flips `true` must not leave the item on a permanent
/// `NotAllowed` cursor. Menu-bar Format/Go rows hit this path — they
/// are built dormant before any editor is attached, then enable when
/// a scene has focus.
#[test]
fn menu_item_cursor_stays_pointer_after_enabled_signal_flips_true() {
    use teksilo_canvas::Point;
    use teksilo_core::widget::CursorIcon;

    let enabled = Signal::new(false);
    let mut t = tree();
    let list_id = t.add(MenuList::new().item(MenuItem::new(lit!("Bold")).enabled(enabled.clone())));
    layout(&mut t);
    let item_id = first_descendant_with_role(&t, list_id, Role::MenuItem);
    let bounds = t.bounds(item_id);
    let center = Point::new(
        bounds.origin().x + bounds.size().width / 2.0,
        bounds.origin().y + bounds.size().height / 2.0,
    );

    // Still disabled at first hover: framework gates PointerEnter, so
    // the item never applies its node_cursor — cursor stays Default.
    t.pointer_move(center);
    // Flip enablement without rebuilding the item (the real menubar
    // path: signals update, paint/AT follow, handlers stay put).
    enabled.set(true);
    // Leave and re-enter so PointerEnter re-applies node_cursor under
    // the now-enabled gate.
    t.pointer_move(Point::new(0.0, 0.0));
    layout(&mut t); // flush effective_enabled + any dirty paint
    t.pointer_move(center);
    assert_eq!(
        t.current_cursor(),
        CursorIcon::Pointer,
        "enabled menu item must show Pointer, not a build-time NotAllowed snapshot"
    );
}

#[test]
fn menu_item_with_shortcut_not_rebuilt_on_unrelated_shortcut_churn() {
    use teksilo_core::event::Key;
    use teksilo_core::shortcut::Shortcut;

    let mut t = tree();
    t.shortcut_registry_mut().register(
        Shortcut::new("test.cmd")
            .primary(KeyStroke::ctrl(Key::K))
            .build(),
    );
    let list_id = t.add(MenuList::new().item(MenuItem::new(lit!("New")).for_shortcut("test.cmd")));
    layout(&mut t);
    let item_id = first_descendant_with_role(&t, list_id, Role::MenuItem);

    // Snapshot the item's subtree identity. A rebuild re-creates the
    // item's children (label / accelerator / chevron) with fresh ids.
    let before = subtree(&t, item_id);

    // Register an UNRELATED shortcut — exactly what a widget that
    // declares a scoped shortcut in build() does on every rebuild —
    // and flush pending rebuilds via layout. The old code bound the
    // GLOBAL shortcut version at `Rebuild` on every shortcut-bearing
    // item, so this bump rebuilt the item, tearing down its gesture
    // arena and dropping in-flight clicks (the reported regression).
    t.shortcut_registry_mut().register(
        Shortcut::new("unrelated.cmd")
            .primary(KeyStroke::ctrl(Key::J))
            .build(),
    );
    layout(&mut t);

    let after = subtree(&t, item_id);
    assert_eq!(
        before, after,
        "a shortcut-bearing menu item must NOT rebuild when an unrelated \
         shortcut is registered; its accelerator now updates as a leaf"
    );
}

// --- Regression: a rebuilt item must not leak its tooltip ---

/// Rebuilding a tooltip-bearing menu item must neither leak the old
/// tooltip's widgets nor lose the tooltip.
///
/// `build()` consumes the tooltip source (`.take()`), so a second build
/// attaches nothing: the entry that survives points at the *previous*
/// build's body, which the rebuild has just destroyed. Every later rebuild
/// then strands one more content subtree — parentless by construction, so
/// no teardown walk can ever reach it — in the arena for the process's
/// lifetime.
#[test]
fn rebuilding_a_menu_item_neither_leaks_nor_loses_its_tooltip() {
    let mut t = tree();
    let list_id = t.add(MenuList::new().item(MenuItem::new(lit!("Bold")).tooltip(lit!("Tip"))));
    layout(&mut t);
    let item_id = first_descendant_with_role(&t, list_id, Role::MenuItem);

    let baseline = t.widget_count();
    for _ in 0..10 {
        t.arena_mark_needs_rebuild_for_testing(item_id);
        layout(&mut t);
        assert_eq!(
            t.tooltip_entry_count(),
            1,
            "the item must keep exactly one tooltip across rebuilds"
        );
    }

    assert_eq!(
        t.widget_count(),
        baseline,
        "each rebuild stranded a tooltip content subtree in the arena"
    );
}

/// **A swatch is not a glyph that repeats the label.**
///
/// A menu icon normally means what the label means, so it takes the row's colour.
/// An icon whose colour *is* the content — a tag's swatch, a status light — has
/// nothing left to say once the row has tinted it to its own foreground. The
/// opt-in leaves it alone; without it, the row wins, which is the default every
/// other row wants.
#[test]
fn an_icon_that_keeps_its_color_is_not_tinted_by_the_row() {
    // A colour no theme role resolves to, so finding it among the painted glyphs
    // can only mean the icon's own was kept.
    let swatch = teksilo_tokens::Color::from_hex("#e91e63");

    let painted = |keep: bool| {
        let mut t = WidgetTree::new()
            .with_theme(teksilo_core::presets::intui::light())
            .with_text_backend(std::rc::Rc::new(std::cell::RefCell::new(
                teksilo_canvas::MockTextBackend::new(),
            )));
        let mut item = MenuItem::new(lit!("Places"))
            .icon(IconWidget::checkmark(MENU_INDICATOR_GLYPH_SIZE).color(swatch));
        if keep {
            item = item.icon_keeps_color();
        }
        t.add(item);
        layout(&mut t);
        // The checkmark is vector artwork, so it lands in `paths` rather than
        // among the label's glyphs.
        t.render()
            .paths
            .iter()
            .map(|p| {
                let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                [q(p.color[0]), q(p.color[1]), q(p.color[2]), q(p.color[3])]
            })
            .collect::<Vec<_>>()
    };

    assert!(
        painted(true).contains(&rgba8(swatch)),
        "the swatch must keep the colour it was given"
    );
    assert!(
        !painted(false).contains(&rgba8(swatch)),
        "and without the opt-in the row must still tint its icon, or every \
         existing menu icon would stop following hover and disabled"
    );
}

/// The same contract for the rich (registry-keyed) tier, which carries a
/// whole Accordion body — ~15 widgets per stranded copy.
#[test]
fn rebuilding_a_menu_item_neither_leaks_nor_loses_its_rich_tooltip() {
    let mut t = tree();
    let list_id =
        t.add(MenuList::new().item(MenuItem::new(lit!("Bold")).rich_tooltip("bold-details")));
    layout(&mut t);
    let item_id = first_descendant_with_role(&t, list_id, Role::MenuItem);

    let baseline = t.widget_count();
    for _ in 0..10 {
        t.arena_mark_needs_rebuild_for_testing(item_id);
        layout(&mut t);
        assert_eq!(t.tooltip_entry_count(), 1);
    }

    assert_eq!(
        t.widget_count(),
        baseline,
        "each rebuild stranded a rich-tooltip content subtree in the arena"
    );
}
