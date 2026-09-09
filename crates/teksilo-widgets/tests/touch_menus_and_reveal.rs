// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What menus, tab close buttons and revealed row actions do under a finger.
//!
//! The mouse side of the same machinery is pinned by
//! `tests/menu_safe_triangle.rs` (the 400 ms hover-open, the 150 ms
//! pointer-leave grace and the 600 ms safe-region budget, all three at once in
//! one of its cases). Nothing here may change any of that; these cases cover
//! the routes a contact has and a mouse does not, plus the two places where
//! choosing the dismissal per open-route also fixes a mouse-visible defect.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use teksilo_canvas::{Point, Rect, SizeProposal};
use teksilo_core::event::Key;
use teksilo_core::presets::intui;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_tokens::TargetDensity;
use teksilo_widgets::menu_item::MenuItem;
use teksilo_widgets::menu_list::MenuList;
use teksilo_widgets::primitives::{FixedSize, VStack};

const MENU_WIDTH: f32 = 220.0;
const VIEWPORT: (f32, f32) = (1024.0, 768.0);

fn layout(tree: &mut WidgetTree) {
    tree.layout(SizeProposal::exact(VIEWPORT.0, VIEWPORT.1));
}

/// A menu whose first row is a submenu trigger. Same shape as the
/// safe-triangle suite's fixture, deliberately.
fn tree_with_menu() -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new().with_theme(intui::light());
    let list = MenuList::new()
        .item(MenuItem::submenu(lit!("More"), || {
            Box::new(
                MenuList::new()
                    .item(MenuItem::new(lit!("Sub One")).on_activate_fn(|_| {}))
                    .item(MenuItem::new(lit!("Sub Two")).on_activate_fn(|_| {})),
            )
        }))
        .item(MenuItem::new(lit!("Second")).on_activate_fn(|_| {}))
        .item(MenuItem::new(lit!("Third")).on_activate_fn(|_| {}));
    let root = tree.add(VStack::new().child(FixedSize::new().width(MENU_WIDTH).child(list)));
    layout(&mut tree);
    (tree, root)
}

fn row_bounds(tree: &WidgetTree, root: WidgetId, index: usize) -> Rect {
    let mut stack = vec![root];
    let mut rows = Vec::new();
    while let Some(id) = stack.pop() {
        for child in tree.children(id) {
            if tree
                .widget_type_name(child)
                .is_some_and(|name| name.contains("MenuItem"))
            {
                rows.push(child);
            }
            stack.push(child);
        }
    }
    rows.sort_by(|a, b| tree.bounds(*a).y.total_cmp(&tree.bounds(*b).y));
    tree.bounds(rows[index])
}

fn centre(rect: Rect) -> Point {
    Point::new(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5)
}

// =====================================================================
// A tap opens a submenu, and it stays
// =====================================================================

#[test]
fn a_tap_opens_a_submenu_and_it_stays_open() {
    let (mut tree, root) = tree_with_menu();
    let trigger = centre(row_bounds(&tree, root, 0));

    let f = tree.new_contact();
    tree.touch_down(f, trigger);
    tree.touch_up(f, trigger);
    layout(&mut tree);
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "a tap on a submenu trigger opens it, with no dwell",
    );

    // A second contact travels across the menu — the closest a finger comes to
    // "leaving" — and the frame passes run.
    let g = tree.new_contact();
    tree.touch_down(g, centre(row_bounds(&tree, root, 2)));
    tree.touch_move(g, Point::new(20.0, 400.0));
    tree.advance_time(Duration::from_millis(500));
    layout(&mut tree);
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "nothing a contact does *leaves* the submenu, so nothing closes it",
    );
}

/// The dismissal is chosen by the route that opened the submenu, not shared —
/// so a pointer that had nothing to do with opening it cannot take it away.
///
/// The hover-owner gate stops a *contact* from starting the pointer-leave
/// grace; it says nothing about a mouse, which is the hover owner and may well
/// be present on the same machine. Without choosing the dismissal per route, a
/// mouse nudged anywhere closes the submenu the finger just opened.
#[test]
fn a_mouse_moving_afterwards_does_not_close_a_tap_opened_submenu() {
    let (mut tree, root) = tree_with_menu();
    let trigger = centre(row_bounds(&tree, root, 0));

    let f = tree.new_contact();
    tree.touch_down(f, trigger);
    tree.touch_up(f, trigger);
    layout(&mut tree);
    assert_eq!(tree.active_overlays().len(), 1);

    // A mouse, far from both the submenu and the row that opened it.
    tree.pointer_move(Point::new(VIEWPORT.0 - 5.0, VIEWPORT.1 - 5.0));
    tree.advance_time(Duration::from_millis(500));
    layout(&mut tree);
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "a pointer that did not open it does not close it",
    );
}

#[test]
fn escape_still_closes_a_tap_opened_submenu() {
    let (mut tree, root) = tree_with_menu();
    let trigger = centre(row_bounds(&tree, root, 0));
    let f = tree.new_contact();
    tree.touch_down(f, trigger);
    tree.touch_up(f, trigger);
    layout(&mut tree);
    assert_eq!(tree.active_overlays().len(), 1);

    tree.press_key(Key::Escape, teksilo_core::event::Modifiers::NONE);
    layout(&mut tree);
    assert!(
        tree.active_overlays().is_empty(),
        "'stays until explicit dismissal' means Escape still works",
    );
}

#[test]
fn a_mouse_click_opened_submenu_still_closes_on_pointer_leave() {
    let (mut tree, root) = tree_with_menu();
    let trigger = centre(row_bounds(&tree, root, 0));
    tree.pointer_move(trigger);
    // Click, without waiting for the 400 ms hover dwell.
    tree.pointer_down_button(trigger, teksilo_core::event::PointerButton::Primary);
    tree.pointer_up_button(trigger, teksilo_core::event::PointerButton::Primary);
    layout(&mut tree);
    assert_eq!(tree.active_overlays().len(), 1);

    // Leave the row and the panel, then let the 150 ms grace run.
    tree.pointer_move(Point::new(10.0, 700.0));
    tree.advance_time(Duration::from_millis(400));
    layout(&mut tree);
    assert!(
        tree.active_overlays().is_empty(),
        "a mouse still has a pointer that can leave, so its click-opened \
         submenu keeps the grace it always had",
    );
}

// =====================================================================
// The pre-existing mouse defect the per-route dismissal fixes
// =====================================================================

/// A keyboard-opened submenu used to be built with the pointer-leave grace, so
/// moving the *mouse* anywhere outside it closed it 150 ms later — a submenu the
/// keyboard user had not finished with, taken away by a pointer they were not
/// using. Fixed by choosing the dismissal from the route that opened it.
#[test]
fn a_keyboard_opened_submenu_survives_a_mouse_move_away() {
    let (mut tree, root) = tree_with_menu();
    let trigger = row_bounds(&tree, root, 0);
    // Focus the trigger row, then open with the inline-forward arrow.
    let row = first_menu_row(&tree, root);
    tree.focus(row);
    tree.press_key(Key::ArrowRight, teksilo_core::event::Modifiers::NONE);
    layout(&mut tree);
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "ArrowRight opens the submenu of the focused trigger",
    );
    let _ = trigger;

    tree.pointer_move(Point::new(10.0, 700.0));
    tree.advance_time(Duration::from_millis(400));
    layout(&mut tree);
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "the keyboard opened it; a mouse that never touched it cannot leave it",
    );

    tree.press_key(Key::Escape, teksilo_core::event::Modifiers::NONE);
    layout(&mut tree);
    assert!(
        tree.active_overlays().is_empty(),
        "and Escape still ends it",
    );
}

fn first_menu_row(tree: &WidgetTree, root: WidgetId) -> WidgetId {
    let mut stack = vec![root];
    let mut rows = Vec::new();
    while let Some(id) = stack.pop() {
        for child in tree.children(id) {
            if tree
                .widget_type_name(child)
                .is_some_and(|name| name.contains("MenuItem"))
            {
                rows.push(child);
            }
            stack.push(child);
        }
    }
    rows.sort_by(|a, b| tree.bounds(*a).y.total_cmp(&tree.bounds(*b).y));
    rows[0]
}

// =====================================================================
// Row 5 of the hover census — the tab close button
// =====================================================================

fn tab_tree(density: TargetDensity) -> (WidgetTree, Rc<Cell<usize>>) {
    use teksilo_widgets::tab_widget::{TabInfo, TabWidget};
    let closed = Rc::new(Cell::new(0usize));
    let counter = closed.clone();
    let mut tree = WidgetTree::new().with_theme(intui::light());
    tree.set_input_density(density);
    let selected = teksilo_core::signal::Signal::new(None);
    let tabs = TabWidget::new(selected)
        .static_tab(
            TabInfo::new().title(lit!("One")).closable(true),
            VStack::new(),
        )
        .static_tab(
            TabInfo::new().title(lit!("Two")).closable(true),
            VStack::new(),
        )
        .on_close(move |_id, _ctx| counter.set(counter.get() + 1));
    tree.add(tabs);
    layout(&mut tree);
    (tree, closed)
}

/// Every widget reachable from the roots, in no particular order.
fn all_ids(tree: &WidgetTree) -> Vec<WidgetId> {
    let mut out = Vec::new();
    let mut stack = tree.roots();
    while let Some(id) = stack.pop() {
        out.push(id);
        stack.extend(tree.children(id));
    }
    out
}

fn ids_named(tree: &WidgetTree, needle: &str) -> Vec<WidgetId> {
    all_ids(tree)
        .into_iter()
        .filter(|id| {
            tree.widget_type_name(*id)
                .is_some_and(|n| n.contains(needle))
        })
        .collect()
}

/// The tab headers the user can see, leading edge first.
///
/// The bar also builds headers it does not show — the overflow dropdown's — so
/// a bare type-name walk picks up a node with no close handler and no bounds,
/// and a test that grabbed the first of those would assert against nothing.
fn visible_headers(tree: &WidgetTree) -> Vec<WidgetId> {
    let mut ids: Vec<WidgetId> = ids_named(tree, "TabHeader")
        .into_iter()
        .filter(|id| tree.is_visible(*id) && tree.bounds(*id).width > 0.0)
        .collect();
    ids.sort_by(|a, b| tree.bounds(*a).x.total_cmp(&tree.bounds(*b).x));
    ids
}

/// The `×` is culled from paint *and* from the accessibility tree by
/// `visible_when`, so "hidden until hovered" and "does not exist" are the same
/// thing for a finger.
fn close_buttons_visible(tree: &WidgetTree) -> usize {
    ids_named(tree, "IconButton")
        .into_iter()
        .filter(|id| tree.is_visible(*id))
        .count()
}

#[test]
fn a_touch_density_shows_every_tab_close_button_at_rest() {
    let (tree, _closed) = tab_tree(TargetDensity::Touch);
    assert!(
        close_buttons_visible(&tree) >= 2,
        "at a density that reveals every affordance the `×` is simply there",
    );
}

/// The companion to the Touch case, and what makes it discriminating: with
/// **zero** icon buttons visible at Compact, the ones counted at Touch cannot be
/// the bar's own scroll arrows or its overflow chevron.
#[test]
fn a_compact_density_hides_the_tab_close_button_at_rest() {
    let (tree, _closed) = tab_tree(TargetDensity::Compact);
    assert_eq!(
        close_buttons_visible(&tree),
        0,
        "the mouse keeps the Firefox/Chrome hover reveal, unchanged",
    );
}

/// The `×` is not focusable and is hover-revealed, so before this an assistive
/// technology had no way at all to close a tab: the advertised custom actions
/// were reorder-only.
#[test]
fn an_at_custom_action_closes_a_tab() {
    use teksilo_core::accesskit::{Action, ActionData};

    let (mut tree, closed) = tab_tree(TargetDensity::Compact);
    let headers = visible_headers(&tree);
    let mut ops = teksilo_core::window::NoopWindowOps;

    // Id 2, explicitly. The list is built conditionally — a tab at index 0
    // advertises no "Move Left" — so a position in the vector says nothing
    // about which action it is.
    let handled = headers
        .iter()
        .filter(|header| {
            let node_id = teksilo_core::accessibility::widget_id_to_node_id(**header);
            tree.dispatch_access_action(
                node_id,
                Action::CustomAction,
                Some(ActionData::CustomAction(2)),
                &mut ops,
            )
        })
        .count();
    assert_eq!(
        handled, 2,
        "each of the two closable tabs offers an AT close"
    );
    assert_eq!(closed.get(), 2, "and each one closes its own tab");

    // …and it is **advertised**, not merely routable. Dispatching id 2 by hand
    // exercises the handler; an assistive technology only ever offers what the
    // node publishes, so a Close that routes but is not in `custom_actions` is
    // a Close no user can reach.
    let (mut tree, _) = tab_tree(TargetDensity::Compact);
    let update = tree.sync_accessibility();
    let advertising = update
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == teksilo_core::accesskit::Role::Tab)
        .filter(|(_, node)| {
            node.custom_actions()
                .iter()
                .any(|action| action.id == 2 && action.description == "Close")
        })
        .count();
    assert_eq!(
        advertising, 2,
        "both closable tabs publish the Close action under its explicit id",
    );
}

/// A tab with no close handler must not advertise the action, or an AT offers a
/// command that does nothing.
#[test]
fn a_non_closable_tab_has_no_at_close_action() {
    use teksilo_core::accesskit::{Action, ActionData};
    use teksilo_widgets::tab_widget::{TabInfo, TabWidget};

    let mut tree = WidgetTree::new().with_theme(intui::light());
    let selected = teksilo_core::signal::Signal::new(None);
    tree.add(
        TabWidget::new(selected)
            .static_tab(TabInfo::new().title(lit!("One")), VStack::new())
            .static_tab(TabInfo::new().title(lit!("Two")), VStack::new()),
    );
    layout(&mut tree);
    let headers = visible_headers(&tree);
    let mut ops = teksilo_core::window::NoopWindowOps;
    for header in headers {
        let node_id = teksilo_core::accessibility::widget_id_to_node_id(header);
        assert!(
            !tree.dispatch_access_action(
                node_id,
                Action::CustomAction,
                Some(ActionData::CustomAction(2)),
                &mut ops,
            ),
            "no close handler, no close action",
        );
    }
}

// =====================================================================
// MenuBar switching — already true on tap, and it must stay true
// =====================================================================

/// A tap opens, toggles and reaches every top-level menu with no hover
/// anywhere — but moving from one open menu to another takes **two** taps, and
/// that is deliberate rather than a gap.
///
/// The trigger's own tap handler does switch (`MenuContext::open_at` dismisses
/// the sibling first), and for a mouse that is what happens. For a direct
/// pointer the press never reaches the trigger: a press outside an open overlay
/// arms that overlay's dismissal and is **withheld from the tree**, so the first
/// tap closes the open menu and the second opens the new one. That rule is not
/// this package's to overturn — it is what stops a finger, which covers what it
/// is about to actuate, from acting on a control it could not see — and it is
/// the same one-tap-to-dismiss convention every touch platform applies to a
/// presented menu.
#[test]
fn a_tap_reaches_every_top_level_menu() {
    use teksilo_widgets::menu_bar::MenuBar;

    let mut tree = WidgetTree::new().with_theme(intui::light());
    let bar = MenuBar::new()
        .menu(lit!("File"), || {
            Box::new(MenuList::new().item(MenuItem::new(lit!("Open")).on_activate_fn(|_| {})))
        })
        .menu(lit!("Edit"), || {
            Box::new(MenuList::new().item(MenuItem::new(lit!("Copy")).on_activate_fn(|_| {})))
        });
    let root = tree.add(VStack::new().child(bar));
    layout(&mut tree);

    let triggers = {
        let mut ids: Vec<WidgetId> = all_ids(&tree)
            .into_iter()
            .filter(|id| {
                tree.widget_type_name(*id)
                    .is_some_and(|n| n.contains("MenuBarTrigger"))
                    && tree.bounds(*id).width > 0.0
            })
            .collect();
        ids.sort_by(|a, b| tree.bounds(*a).x.total_cmp(&tree.bounds(*b).x));
        ids
    };
    assert_eq!(triggers.len(), 2, "two top-level menus, two triggers");
    let _ = root;

    let tap = |tree: &mut WidgetTree, at: Point| {
        let f = tree.new_contact();
        tree.touch_down(f, at);
        tree.touch_up(f, at);
        layout(tree);
    };

    let first = centre(tree.bounds(triggers[0]));
    let second = centre(tree.bounds(triggers[1]));

    tap(&mut tree, first);
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "a tap opens the first menu"
    );
    assert!(
        tree.find_by_label("Open").is_some(),
        "and it is the File menu",
    );

    tap(&mut tree, first);
    assert!(
        tree.active_overlays().is_empty(),
        "a tap on the open trigger closes it — the toggle, with no hover",
    );

    tap(&mut tree, second);
    assert!(
        tree.find_by_label("Copy").is_some(),
        "a tap on the other trigger opens the other menu",
    );

    // …and from an open menu it takes two: the first tap is the open menu's
    // outside-press dismissal, withheld from the tree.
    tap(&mut tree, first);
    assert!(
        tree.active_overlays().is_empty(),
        "the first tap outside the open menu dismisses it, and nothing else",
    );
    tap(&mut tree, first);
    assert!(
        tree.find_by_label("Open").is_some(),
        "the second tap opens the menu that was tapped",
    );
}

// =====================================================================
// Row 11 of the hover census — the row-action reveal contract
// =====================================================================

fn reveal_for(density: TargetDensity) -> (WidgetTree, teksilo_core::signal::Signal<bool>) {
    use teksilo_widgets::StandardListItem;

    let revealed = teksilo_core::signal::Signal::new(false);
    let mut tree = WidgetTree::new().with_theme(intui::light());
    tree.set_input_density(density);
    tree.add(
        VStack::new().child(StandardListItem::new(lit!("A row")).reveal_signal(revealed.clone())),
    );
    layout(&mut tree);
    (tree, revealed)
}

#[test]
fn a_touch_density_reveals_row_actions_at_rest() {
    let (_tree, revealed) = reveal_for(TargetDensity::Touch);
    assert!(
        revealed.get(),
        "a finger produces no hover, so a row gated on hover has no actions at all",
    );
}

#[test]
fn a_compact_density_reveals_row_actions_only_on_hover() {
    let (mut tree, revealed) = reveal_for(TargetDensity::Compact);
    assert!(!revealed.get(), "the mouse contract is unchanged");

    let row = ids_named(&tree, "StandardListItem")[0];
    let at = centre(tree.bounds(row));
    tree.pointer_move(at);
    assert!(revealed.get(), "hovering the row reveals its actions");

    tree.pointer_move(Point::new(2.0, 700.0));
    assert!(!revealed.get(), "and leaving it hides them again");
}

/// The reveal is a *separate* question from the interaction state, and has to
/// be: forcing the interaction signal to `Hovered` at Touch would leave every
/// row permanently tinted.
#[test]
fn a_touch_density_does_not_permanently_hover_the_row() {
    use teksilo_widgets::StandardListItem;
    use teksilo_widgets::button::InteractionState;

    let interaction = teksilo_core::signal::Signal::new(InteractionState::Idle);
    let revealed = teksilo_core::signal::Signal::new(false);
    let mut tree = WidgetTree::new().with_theme(intui::light());
    tree.set_input_density(TargetDensity::Touch);
    tree.add(
        VStack::new().child(
            StandardListItem::new(lit!("A row"))
                .interaction_signal(interaction.clone())
                .reveal_signal(revealed.clone()),
        ),
    );
    layout(&mut tree);

    assert!(revealed.get());
    assert_eq!(
        interaction.get(),
        InteractionState::Idle,
        "the row is not hovered, and must not claim to be",
    );
}

// =====================================================================
// Snackbar — swipe dismissal, and the pause that is exempt
// =====================================================================

/// The snackbar's only user-driven exits were Escape (which needs focus the
/// surface never takes) and a press outside — which under a finger means aiming
/// at whatever is behind it. A swipe across the surface needs neither.
#[test]
fn a_swipe_dismisses_a_snackbar_and_a_mouse_drag_does_not() {
    use teksilo_widgets::primitives::TextWidget;
    use teksilo_widgets::snackbar::Snackbar;

    let swipe = teksilo_tokens::GestureProfile::TOUCH.swipe_min_distance + 40.0;

    let present = |tree: &mut WidgetTree, root: WidgetId| {
        // The default trigger is a Button; tap it to present the surface.
        let trigger = ids_named(tree, "Button")
            .into_iter()
            .find(|id| tree.bounds(*id).width > 0.0)
            .expect("the snackbar builds a default Button trigger");
        let at = centre(tree.bounds(trigger));
        let f = tree.new_contact();
        tree.touch_down(f, at);
        tree.touch_up(f, at);
        layout(tree);
        let _ = root;
    };

    let build = || {
        let mut tree = WidgetTree::new().with_theme(intui::light());
        let root = tree
            .add(VStack::new().child(
                Snackbar::new(lit!("Undo")).content(TextWidget::new(lit!("File deleted."))),
            ));
        layout(&mut tree);
        (tree, root)
    };

    // A finger.
    let (mut tree, root) = build();
    present(&mut tree, root);
    assert_eq!(tree.active_overlays().len(), 1, "the trigger presents it");
    let surface = centre(
        tree.overlay_manager()
            .bounds_for_content(tree.overlay_manager().active_content_ids()[0])
            .expect("an open overlay has bounds after a layout pass"),
    );
    let f = tree.new_contact();
    tree.touch_down(f, surface);
    tree.touch_move(f, Point::new(surface.x + swipe * 0.5, surface.y));
    tree.advance_time(Duration::from_millis(16));
    tree.touch_move(f, Point::new(surface.x + swipe, surface.y));
    tree.touch_up(f, Point::new(surface.x + swipe, surface.y));
    layout(&mut tree);
    assert!(
        tree.active_overlays().is_empty(),
        "a horizontal swipe dismisses the snackbar",
    );

    // A mouse doing the same thing, fast.
    let (mut tree, root) = build();
    present(&mut tree, root);
    let surface = centre(
        tree.overlay_manager()
            .bounds_for_content(tree.overlay_manager().active_content_ids()[0])
            .expect("an open overlay has bounds after a layout pass"),
    );
    tree.pointer_move(surface);
    tree.pointer_down_button(surface, teksilo_core::event::PointerButton::Primary);
    tree.pointer_move(Point::new(surface.x + swipe * 0.5, surface.y));
    tree.advance_time(Duration::from_millis(16));
    tree.pointer_move(Point::new(surface.x + swipe, surface.y));
    tree.pointer_up_button(
        Point::new(surface.x + swipe, surface.y),
        teksilo_core::event::PointerButton::Primary,
    );
    layout(&mut tree);
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "a mouse drag across the panel is not a dismissal request",
    );
}
