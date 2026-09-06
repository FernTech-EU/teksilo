// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Integration tests for the safe-triangle submenu hover gate.
//!
//! The geometry itself is unit-tested in
//! `teksilo_core::overlay::safe_triangle`. What these cover is the
//! wiring, which is where the gate is easy to get wrong: an open
//! submenu can be torn down by *two* independent paths — a sibling
//! row's hover-switch, and the overlay's own `PointerLeave` grace —
//! and gating only the first leaves the traversal on a 150 ms clock.

use std::time::Duration;

use teksilo_canvas::{Point, Rect, SizeProposal};
use teksilo_core::presets::intui;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_widgets::menu_item::MenuItem;
use teksilo_widgets::menu_list::MenuList;
use teksilo_widgets::primitives::{FixedSize, VStack};

/// Menu width, chosen so the submenu has room to open on the trailing
/// edge (a full-width list would flip it to the leading side).
const MENU_WIDTH: f32 = 220.0;

/// A menu whose first row is a submenu trigger, followed by three
/// plain rows — or, with `second_is_submenu`, a second trigger in row 1.
fn tree_with_menu(second_is_submenu: bool) -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new().with_theme(intui::light());
    let mut list = MenuList::new().item(MenuItem::submenu(lit!("More"), || {
        Box::new(
            MenuList::new()
                .item(MenuItem::new(lit!("Sub One")).on_activate_fn(|_| {}))
                .item(MenuItem::new(lit!("Sub Two")).on_activate_fn(|_| {}))
                .item(MenuItem::new(lit!("Sub Three")).on_activate_fn(|_| {})),
        )
    }));
    list = if second_is_submenu {
        list.item(MenuItem::submenu(lit!("Other"), || {
            Box::new(MenuList::new().item(MenuItem::new(lit!("X")).on_activate_fn(|_| {})))
        }))
    } else {
        list.item(MenuItem::new(lit!("Second")).on_activate_fn(|_| {}))
    };
    list = list
        .item(MenuItem::new(lit!("Third")).on_activate_fn(|_| {}))
        .item(MenuItem::new(lit!("Fourth")).on_activate_fn(|_| {}));
    let root = tree.add(VStack::new().child(FixedSize::new().width(MENU_WIDTH).child(list)));
    tree.layout(SizeProposal::exact(1024.0, 768.0));
    (tree, root)
}

/// Bounds of the `index`-th row, in top-to-bottom visual order.
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

/// Hover the trigger, let the open delay elapse, and return the
/// submenu's rect plus the pointer position (which is the apex the
/// gate will use once the pointer leaves the row).
fn open_submenu(tree: &mut WidgetTree, root: WidgetId) -> (Rect, Point) {
    let trigger = row_bounds(tree, root, 0);
    let apex = Point::new(
        trigger.x + trigger.width * 0.5,
        trigger.y + trigger.height * 0.5,
    );
    tree.pointer_move(apex);
    tree.advance_time(Duration::from_millis(450));
    tree.layout(SizeProposal::exact(1024.0, 768.0));
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "the submenu should be open once the hover delay elapsed"
    );
    let content = tree.overlay_manager().active_content_ids()[0];
    let bounds = tree
        .overlay_manager()
        .bounds_for_content(content)
        .expect("an open overlay has bounds after a layout pass");
    assert!(
        bounds.width > 0.0 && bounds.x >= trigger.x + trigger.width,
        "the submenu should open on the trailing edge, got {bounds:?}"
    );
    (bounds, apex)
}

/// The same point-in-triangle test the implementation runs, so each
/// case can assert its own precondition rather than trusting a
/// hand-picked coordinate to be inside the cone.
fn in_cone(point: Point, apex: Point, submenu: Rect) -> bool {
    let near_x = if apex.x < submenu.x {
        submenu.x
    } else if apex.x > submenu.x + submenu.width {
        submenu.x + submenu.width
    } else {
        return false;
    };
    let corners = [
        apex,
        Point::new(near_x, submenu.y),
        Point::new(near_x, submenu.y + submenu.height),
    ];
    let cross =
        |p: Point, a: Point, b: Point| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
    let signs = [
        cross(point, corners[0], corners[1]),
        cross(point, corners[1], corners[2]),
        cross(point, corners[2], corners[0]),
    ];
    !(signs.iter().any(|s| *s < 0.0) && signs.iter().any(|s| *s > 0.0))
}

/// A point on row `index`, near its trailing edge — i.e. on the
/// diagonal a user actually walks toward a submenu on the right.
fn point_on_row(tree: &WidgetTree, root: WidgetId, index: usize) -> Point {
    let row = row_bounds(tree, root, index);
    Point::new(row.x + row.width * 0.95, row.y + row.height * 0.5)
}

#[test]
fn a_plain_sibling_hover_inside_the_cone_keeps_the_submenu_open() {
    let (mut tree, root) = tree_with_menu(false);
    let (submenu, apex) = open_submenu(&mut tree, root);
    let point = point_on_row(&tree, root, 1);
    assert!(in_cone(point, apex, submenu), "precondition: {point:?}");

    tree.pointer_move(point);
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "a sibling row crossed on the way to the submenu must not dismiss it"
    );
}

#[test]
fn the_pointer_leave_grace_does_not_run_while_inside_the_cone() {
    let (mut tree, root) = tree_with_menu(false);
    let (submenu, apex) = open_submenu(&mut tree, root);
    let point = point_on_row(&tree, root, 1);
    assert!(in_cone(point, apex, submenu), "precondition: {point:?}");

    tree.pointer_move(point);
    // Far longer than the 150 ms submenu pointer-leave grace: a
    // deliberate, slow diagonal. This is the half of the gate that
    // sibling-hover suppression alone never covered.
    tree.advance_time(Duration::from_millis(400));
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "the pointer-leave grace must be held off while the pointer travels the triangle"
    );
}

#[test]
fn a_sibling_submenu_trigger_inside_the_cone_keeps_the_submenu_open() {
    let (mut tree, root) = tree_with_menu(true);
    let (submenu, apex) = open_submenu(&mut tree, root);
    let point = point_on_row(&tree, root, 1);
    assert!(in_cone(point, apex, submenu), "precondition: {point:?}");

    tree.pointer_move(point);
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "crossing a *neighbouring submenu trigger* must not dismiss the open submenu either"
    );
}

#[test]
fn settling_on_a_sibling_submenu_trigger_swaps_the_submenu() {
    let (mut tree, root) = tree_with_menu(true);
    let (_, _) = open_submenu(&mut tree, root);
    let first = tree.overlay_manager().active_content_ids()[0];

    tree.pointer_move(point_on_row(&tree, root, 1));
    // The user stops here: the deferred hover-switch fires, and the
    // swap has to be a swap — the old submenu goes as the new one opens.
    tree.advance_time(Duration::from_millis(450));
    tree.layout(SizeProposal::exact(1024.0, 768.0));
    let open = tree.overlay_manager().active_content_ids();
    assert_eq!(open.len(), 1, "exactly one submenu open, got {open:?}");
    assert_ne!(
        open[0], first,
        "the neighbour's submenu should have replaced it"
    );
}

#[test]
fn a_pointer_parked_inside_the_cone_releases_the_submenu() {
    let (mut tree, root) = tree_with_menu(false);
    let (submenu, apex) = open_submenu(&mut tree, root);
    let point = point_on_row(&tree, root, 1);
    assert!(in_cone(point, apex, submenu), "precondition: {point:?}");

    tree.pointer_move(point);
    // The safe region is a travel allowance, not a pin: a pointer that
    // stops moving is no longer travelling, so once the budget is spent
    // the ordinary grace resumes and the submenu closes. Two ticks
    // because those are two phases — expiry, then the 150 ms grace.
    tree.advance_time(Duration::from_millis(700));
    tree.advance_time(Duration::from_millis(200));
    assert!(
        tree.active_overlays().is_empty(),
        "the safe region must expire rather than hold the submenu open forever"
    );
}

#[test]
fn a_sibling_hover_outside_the_cone_dismisses_immediately() {
    let (mut tree, root) = tree_with_menu(false);
    let (submenu, apex) = open_submenu(&mut tree, root);
    // The *leading* edge of the last row: away from the submenu, well
    // outside the cone. Walking here means the user changed their mind.
    let row = row_bounds(&tree, root, 3);
    let point = Point::new(row.x + 4.0, row.y + row.height * 0.5);
    assert!(!in_cone(point, apex, submenu), "precondition: {point:?}");

    tree.pointer_move(point);
    assert!(
        tree.active_overlays().is_empty(),
        "leaving the cone must still dismiss the submenu at once"
    );
}
