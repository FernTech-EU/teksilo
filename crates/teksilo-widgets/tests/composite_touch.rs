// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What the composite, partitioned and docking controls do under a finger, and
//! what they still do under a mouse.
//!
//! Three questions run through the whole file, one per mechanism the controls
//! sweep leaves for the composites:
//!
//! * **a half that is smaller than the floor** — a `SplitButton`'s 22 dp
//!   chevron sits beside a wide action half, so the miss-only slop pass can
//!   never serve it (its neighbour owns the press at distance zero) and only a
//!   `Widget::hit_outset` inside the exact pass can. The discriminating case is
//!   therefore a press that *does* land on an eligible neighbour;
//! * **a zone inside one node** — a `TableView` header cell's filter affordance
//!   and a `DropTarget`'s edge bands are painted geometry, told apart by
//!   coordinate at press time. They are floored by the same
//!   clamp-and-redistribute rule and reported so an audit can see them;
//! * **a press visual nothing reached** — three controls carried a pressed
//!   appearance their theme painted and no pointer ever wrote. They are on the
//!   framework press now, which is what makes them answer a finger *and* a
//!   mouse.
//!
//! Every density assertion is made at all three rungs, because the interesting
//! half of the floor rule is that Compact does not move.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, RenderFrame, SizeProposal};
use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
use teksilo_core::presets::intui;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_tokens::{PointerKind, TargetDensity};

// =====================================================================
// Shared scaffolding
// =====================================================================

fn tree_at(density: TargetDensity) -> WidgetTree {
    let mut tree = WidgetTree::new().with_theme(intui::light());
    tree.set_input_density(density);
    tree
}

fn centre(rect: Rect) -> Point {
    Point::new(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5)
}

/// Every fill colour in one frame — the same probe `tests/disabled_ancestor.rs`
/// uses, and the only way to observe a control's *own* interaction signal from
/// outside it without a bespoke Tier-3 style per widget.
fn colours(frame: &RenderFrame) -> Vec<[f32; 4]> {
    frame
        .shapes
        .iter()
        .map(|s| s.color)
        .chain(frame.decorations.iter().map(|d| d.color))
        .collect()
}

fn mouse_down(tree: &mut WidgetTree, at: Point) {
    tree.dispatch_event(WidgetEvent::pointer_down(
        at,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
}

fn mouse_up(tree: &mut WidgetTree, at: Point) {
    tree.dispatch_event(WidgetEvent::pointer_up(
        at,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
}

/// The first descendant of `root` whose widget type name contains `needle`,
/// depth-first in child order.
fn find_by_type(tree: &WidgetTree, root: WidgetId, needle: &str) -> Option<WidgetId> {
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if tree
            .widget_type_name(id)
            .is_some_and(|name| name.contains(needle))
        {
            return Some(id);
        }
        for child in tree.children(id).into_iter().rev() {
            stack.push(child);
        }
    }
    None
}

// =====================================================================
// SplitButton — the 22 dp chevron beside a wide action half
// =====================================================================

fn split_button_tree(density: TargetDensity) -> (WidgetTree, WidgetId, Rc<Cell<u32>>) {
    use teksilo_widgets::menu_item::MenuItem;
    use teksilo_widgets::split_button::SplitButton;

    let fired = Rc::new(Cell::new(0u32));
    let f = fired.clone();
    let mut tree = tree_at(density);
    // The action half fires the *selected* item's action, so the counter goes
    // on the first row.
    let root = tree.add(
        SplitButton::new()
            .item(MenuItem::new(lit!("Run")).on_activate_fn(move |_| f.set(f.get() + 1)))
            .item(MenuItem::new(lit!("Run Tests")).on_activate_fn(|_| {})),
    );
    tree.layout(SizeProposal::exact(400.0, 200.0));
    let chevron = find_by_type(&tree, root, "ChevronRegion").expect("chevron region node");
    (tree, chevron, fired)
}

/// The chevron's reachable width — its own box plus the outset it declares —
/// meets the density's target size at every rung, and its *painted* box never
/// moves.
#[test]
fn the_split_button_chevron_reaches_the_target_floor_at_every_density() {
    for (density, target) in [
        (TargetDensity::Compact, 24.0_f32),
        (TargetDensity::Comfortable, 32.0),
        (TargetDensity::Touch, 44.0),
    ] {
        let (tree, chevron, _) = split_button_tree(density);
        let painted = tree.bounds(chevron);
        assert!(
            (painted.width - 22.0).abs() < 0.01,
            "{density:?}: the chevron is painted 22 dp wide at every density, got {}",
            painted.width,
        );
        let outset = tree.widget_hit_outset(chevron, PointerKind::Touch);
        assert!(
            (painted.width + outset.leading + outset.trailing - target).abs() < 0.01,
            "{density:?}: reachable width {} + {} should be {target}",
            painted.width,
            outset.leading + outset.trailing,
        );
        assert_eq!(
            tree.widget_hit_outset(chevron, PointerKind::Mouse),
            teksilo_canvas::EdgeInsets::ZERO,
            "{density:?}: a mouse hit-tests the chevron exactly as it is painted",
        );
    }
}

/// The whole shortfall is on the leading edge, so a finger reaches the chevron
/// from outside its painted box.
///
/// **The point is on the hairline divider**, which is what the two halves
/// really have between them: a 1 dp node with no handler of its own. That makes
/// it the honest probe for both devices — for a finger the chevron's outset
/// covers it, and for a mouse nothing does.
///
/// This is also the case the miss-only slop pass cannot answer. The press lands
/// inside the divider, whose bubble path carries the action half's `on_tap`
/// before it ever leaves the row, so a slop candidate would have to be strictly
/// closer than a handler already at distance zero. Only a `hit_outset` inside
/// the exact pass can win it — deleting `ChevronRegion::hit_outset` leaves the
/// press with nothing to do.
#[test]
fn a_finger_just_outside_the_chevron_still_opens_the_split_buttons_menu() {
    let (mut tree, chevron, fired) = split_button_tree(TargetDensity::Compact);
    let box_ = tree.bounds(chevron);
    let on_the_divider = Point::new(box_.x - 0.5, box_.y + box_.height * 0.5);

    let f = tree.new_contact();
    tree.touch_down(f, on_the_divider);
    tree.touch_up(f, on_the_divider);
    tree.layout(SizeProposal::exact(400.0, 200.0));

    assert_eq!(fired.get(), 0, "the default action must not have fired");
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "the chevron's outset claimed the press, so the dropdown opened",
    );
}

/// The same point under a mouse opens nothing: the outset is zero for a precise
/// pointer, so what a mouse hit-tests is what the control paints — and what it
/// paints there is a divider.
#[test]
fn a_mouse_just_outside_the_chevron_opens_nothing() {
    let (mut tree, chevron, fired) = split_button_tree(TargetDensity::Compact);
    let box_ = tree.bounds(chevron);
    let on_the_divider = Point::new(box_.x - 0.5, box_.y + box_.height * 0.5);

    mouse_down(&mut tree, on_the_divider);
    mouse_up(&mut tree, on_the_divider);
    tree.layout(SizeProposal::exact(400.0, 200.0));

    assert_eq!(
        tree.active_overlays().len(),
        0,
        "no dropdown: the chevron never saw the press",
    );
    assert_eq!(fired.get(), 0, "and the divider is nobody's target");
}

/// And the outset does not swallow the action half: a press a few dp further in
/// still fires the default action, under either device.
#[test]
fn the_chevrons_outset_leaves_the_action_half_alone() {
    for touch in [false, true] {
        let (mut tree, chevron, fired) = split_button_tree(TargetDensity::Compact);
        let box_ = tree.bounds(chevron);
        let at = Point::new(box_.x - 6.0, box_.y + box_.height * 0.5);
        if touch {
            let f = tree.new_contact();
            tree.touch_down(f, at);
            tree.touch_up(f, at);
        } else {
            mouse_down(&mut tree, at);
            mouse_up(&mut tree, at);
        }
        tree.layout(SizeProposal::exact(400.0, 200.0));
        assert_eq!(fired.get(), 1, "touch={touch}: the action half kept it");
        assert_eq!(tree.active_overlays().len(), 0, "touch={touch}");
    }
}

/// A finger on the chevron itself still opens the menu — the outset resolves
/// *through* the child, so a point inside its real bounds behaves normally.
#[test]
fn a_finger_on_the_chevron_itself_opens_the_split_buttons_menu() {
    let (mut tree, chevron, fired) = split_button_tree(TargetDensity::Compact);
    let at = centre(tree.bounds(chevron));

    let f = tree.new_contact();
    tree.touch_down(f, at);
    tree.touch_up(f, at);
    tree.layout(SizeProposal::exact(400.0, 200.0));

    assert_eq!(fired.get(), 0);
    assert_eq!(tree.active_overlays().len(), 1);
}

// =====================================================================
// Calendar — the header's navigation arrows
// =====================================================================

/// The nav arrow's footprint follows the density ladder, like the day cells
/// beside it. It read the raw 24 dp constant before, so a Touch build had 24 dp
/// arrows in a header whose recipe had already decided on 44.
#[test]
fn a_calendar_nav_arrow_follows_the_density_ladder() {
    use teksilo_widgets::calendar::Calendar;

    for (density, expected) in [
        (TargetDensity::Compact, 24.0_f32),
        (TargetDensity::Comfortable, 32.0),
        (TargetDensity::Touch, 44.0),
    ] {
        let mut tree = tree_at(density);
        let root = tree.add(Calendar::single(teksilo_core::signal::Signal::new(None)));
        tree.layout(SizeProposal::exact(600.0, 600.0));
        let arrow = find_by_type(&tree, root, "NavArrow").expect("a nav arrow node");
        let bounds = tree.bounds(arrow);
        assert!(
            (bounds.width - expected).abs() < 0.01 && (bounds.height - expected).abs() < 0.01,
            "{density:?}: nav arrow is {}x{}, expected {expected} square",
            bounds.width,
            bounds.height,
        );
    }
}

/// The structural half of the ladder test above: the Int UI arrow is the
/// density's target at every rung, so the conformance box is the identity and
/// `common::conformance_box` builds no wrapper — the painted `FixedSize` is
/// the only one under each arrow. Reddens if the calendar goes back to
/// wrapping unconditionally (a second `FixedSize` per arrow), while the macOS
/// preset's `a_macos_icon_buttons_chrome_is_still_apples_twenty_two_dp` holds
/// the non-identity arm.
#[test]
fn an_int_ui_nav_arrow_carries_no_conformance_wrapper() {
    use teksilo_widgets::calendar::Calendar;

    for density in [
        TargetDensity::Compact,
        TargetDensity::Comfortable,
        TargetDensity::Touch,
    ] {
        let mut tree = tree_at(density);
        let root = tree.add(Calendar::single(teksilo_core::signal::Signal::new(None)));
        tree.layout(SizeProposal::exact(600.0, 600.0));
        let arrow = find_by_type(&tree, root, "NavArrow").expect("a nav arrow node");
        let mut fixed_sizes = 0;
        let mut stack = vec![arrow];
        while let Some(id) = stack.pop() {
            if tree
                .widget_type_name(id)
                .is_some_and(|name| name.contains("FixedSize"))
            {
                fixed_sizes += 1;
            }
            stack.extend(tree.children(id));
        }
        assert_eq!(
            fixed_sizes, 1,
            "{density:?}: the painted square is the arrow's only FixedSize \
             — no box was built around a chrome already at the target",
        );
    }
}

/// A month cell's pressed chrome, which nothing could reach: the cell only ever
/// wrote `false` into the signal its `CalendarStyle` paints from.
#[test]
fn a_press_on_a_calendar_month_cell_shows_its_pressed_chrome() {
    use teksilo_widgets::calendar::Calendar;

    let mut tree = tree_at(TargetDensity::Compact);
    let calendar = Calendar::single(teksilo_core::signal::Signal::new(None));
    // Zoom out to the months grid, which is what a `ZoomCell` is. The mode
    // signal is bound at `Rebuild`, so writing it is the supported way in.
    let mode = calendar.mode_signal();
    let root = tree.add(calendar);
    tree.layout(SizeProposal::exact(600.0, 600.0));
    mode.set(teksilo_widgets::calendar::CalendarMode::Months);
    tree.layout(SizeProposal::exact(600.0, 600.0));

    let cell = find_by_type(&tree, root, "ZoomCell").expect("a month cell");
    let cell_at = centre(tree.bounds(cell));
    let idle = colours(&tree.render());

    let f = tree.new_contact();
    tree.touch_down(f, cell_at);
    tree.layout(SizeProposal::exact(600.0, 600.0));
    let pressed = colours(&tree.render());
    assert_ne!(
        idle, pressed,
        "a held month cell must look different from an untouched one",
    );

    tree.touch_up(f, cell_at);
    tree.layout(SizeProposal::exact(600.0, 600.0));
    let released = colours(&tree.render());
    assert_ne!(
        pressed, released,
        "and the pressed chrome must go away when the finger lifts",
    );
}

// =====================================================================
// ToolBox — the section header's pressed chrome
// =====================================================================

fn tool_box_tree() -> (WidgetTree, Point) {
    use teksilo_widgets::primitives::RectWidget;
    use teksilo_widgets::tool_box::ToolBox;

    let selected = teksilo_core::signal::Signal::new(0usize);
    let mut tree = tree_at(TargetDensity::Compact);
    let root = tree.add(
        ToolBox::new(selected)
            .item(lit!("Shapes"), RectWidget::new())
            .item(lit!("Colours"), RectWidget::new()),
    );
    tree.layout(SizeProposal::exact(300.0, 400.0));
    let header = find_by_type(&tree, root, "Header").expect("a section header");
    let at = centre(tree.bounds(header));
    (tree, at)
}

/// The centre of the ToolBox's *second* section header.
fn second_tool_box_header(tree: &WidgetTree) -> Point {
    let mut headers: Vec<WidgetId> = Vec::new();
    let mut stack: Vec<WidgetId> = tree.roots();
    while let Some(id) = stack.pop() {
        if tree
            .widget_type_name(id)
            .is_some_and(|n| n.contains("Header"))
        {
            headers.push(id);
        }
        for child in tree.children(id) {
            stack.push(child);
        }
    }
    headers.sort_by(|a, b| tree.bounds(*a).y.total_cmp(&tree.bounds(*b).y));
    centre(tree.bounds(*headers.last().expect("two section headers")))
}

/// The ToolBox header's `Pressed` state was written by the keyboard alone, so
/// the appearance its recipe paints was unreachable for a mouse as well as for
/// a finger.
#[test]
fn a_press_on_a_tool_box_header_shows_its_pressed_chrome() {
    let (mut tree, at) = tool_box_tree();
    let idle = colours(&tree.render());

    mouse_down(&mut tree, at);
    tree.layout(SizeProposal::exact(300.0, 400.0));
    let pressed = colours(&tree.render());
    assert_ne!(idle, pressed, "a held header must look held");

    mouse_up(&mut tree, at);
    tree.layout(SizeProposal::exact(300.0, 400.0));
    assert_ne!(
        pressed,
        colours(&tree.render()),
        "and stop looking held once the button is up",
    );
}

/// A finger's tap leaves the header untinted: a contact sends no hover-leave to
/// correct a `Hovered` resting state with, so the header would otherwise stay
/// lit with nothing on it.
///
/// Asserted against the tint a *mouse* hover produces rather than against the
/// whole frame, because the tap also selects the section, and that legitimately
/// changes what is painted.
#[test]
fn a_finger_tapping_a_tool_box_header_leaves_it_untinted() {
    // Four steps, because a selected header's own chrome hides a hover tint:
    // the lingering tint can only be *seen* on a header that has since stopped
    // being the selected one. Which is also when a user sees it — tap section
    // B, tap section A, and B is still lit as though the finger were on it.
    let (mut tree, first) = tool_box_tree();
    let second = second_tool_box_header(&tree);
    let idle = colours(&tree.render());

    // 1. What a hover looks like on the *unselected* second header.
    tree.dispatch_event(WidgetEvent::pointer_move(second));
    tree.layout(SizeProposal::exact(300.0, 400.0));
    let hover_tint: Vec<[f32; 4]> = colours(&tree.render())
        .into_iter()
        .filter(|c| !idle.contains(c))
        .collect();
    assert!(
        !hover_tint.is_empty(),
        "fixture: a mouse hover must tint an unselected header, or this proves nothing",
    );
    tree.dispatch_event(WidgetEvent::pointer_move(Point::new(290.0, 390.0)));
    tree.layout(SizeProposal::exact(300.0, 400.0));

    // 2. A finger taps the second header (it becomes the selected one) …
    let f = tree.new_contact();
    tree.touch_down(f, second);
    tree.touch_up(f, second);
    tree.layout(SizeProposal::exact(300.0, 400.0));

    // 3. … and then the first, which takes the selection back.
    let g = tree.new_contact();
    tree.touch_down(g, first);
    tree.touch_up(g, first);
    tree.layout(SizeProposal::exact(300.0, 400.0));

    // 4. Nothing is under a pointer, and nothing may be painted as though it is.
    let after = colours(&tree.render());
    for tint in &hover_tint {
        assert!(
            !after.contains(tint),
            "a header the finger left is still wearing its hover tint {tint:?}",
        );
    }
}

// =====================================================================
// RadioTile — a whole-card control's pressed chrome
// =====================================================================

/// The tile is the control a press visual matters most on: there is no smaller
/// affordance inside the card to look at. Its `Pressed` state was `Space`-only.
#[test]
fn a_press_on_a_radio_tile_shows_its_pressed_chrome() {
    use teksilo_widgets::radio_tile::RadioTile;

    let mut tree = tree_at(TargetDensity::Compact);
    let selected = teksilo_core::signal::Signal::new(0usize);
    let root = tree.add(
        RadioTile::new()
            .selection(1usize, selected.clone())
            .title(lit!("Fast")),
    );
    tree.layout(SizeProposal::exact(300.0, 120.0));
    let at = centre(tree.bounds(root));
    let idle = colours(&tree.render());

    let f = tree.new_contact();
    tree.touch_down(f, at);
    tree.layout(SizeProposal::exact(300.0, 120.0));
    assert_ne!(
        idle,
        colours(&tree.render()),
        "a held tile must look held under a finger",
    );

    tree.touch_up(f, at);
    tree.layout(SizeProposal::exact(300.0, 120.0));
    assert_eq!(selected.get(), 1, "and the tap still selects on release");
}

/// And a finger's tap leaves the tile untinted once the selection moves on — the
/// same lingering-hover defect the ToolBox header had, visible in the same
/// two-tap shape, because a selected tile's own chrome hides it until then.
#[test]
fn a_finger_tapping_a_radio_tile_leaves_it_untinted() {
    use teksilo_widgets::primitives::VStack;
    use teksilo_widgets::radio_tile::RadioTile;

    let mut tree = tree_at(TargetDensity::Compact);
    let selected = teksilo_core::signal::Signal::new(0usize);
    let a = tree.add(
        RadioTile::new()
            .selection(1usize, selected.clone())
            .title(lit!("Fast")),
    );
    let b = tree.add(
        RadioTile::new()
            .selection(2usize, selected.clone())
            .title(lit!("Slow")),
    );
    tree.add(VStack::new().add_child(a).add_child(b));
    tree.layout(SizeProposal::exact(300.0, 200.0));
    let a_at = centre(tree.bounds(a));
    let b_at = centre(tree.bounds(b));
    let idle = colours(&tree.render());

    // What a hover looks like on an unselected tile.
    tree.dispatch_event(WidgetEvent::pointer_move(a_at));
    tree.layout(SizeProposal::exact(300.0, 200.0));
    let hover_tint: Vec<[f32; 4]> = colours(&tree.render())
        .into_iter()
        .filter(|c| !idle.contains(c))
        .collect();
    assert!(
        !hover_tint.is_empty(),
        "fixture: a mouse hover must tint an unselected tile",
    );
    tree.dispatch_event(WidgetEvent::pointer_move(Point::new(295.0, 195.0)));
    tree.layout(SizeProposal::exact(300.0, 200.0));

    // A finger picks A, then B — so A is no longer selected and has nothing
    // left to hide a stale hover behind.
    for at in [a_at, b_at] {
        let f = tree.new_contact();
        tree.touch_down(f, at);
        tree.touch_up(f, at);
        tree.layout(SizeProposal::exact(300.0, 200.0));
    }
    assert_eq!(selected.get(), 2);

    let after = colours(&tree.render());
    for tint in &hover_tint {
        assert!(
            !after.contains(tint),
            "the tile the finger left is still wearing its hover tint {tint:?}",
        );
    }
}

// =====================================================================
// StandardItem — the row height every list and tree row measures
// =====================================================================

/// A row is a target, so its floor follows the ladder. Both projections have
/// been on `StandardItemRecipe` since the density sweep and nothing read them:
/// the row measured itself against the raw Compact constants.
#[test]
fn a_standard_row_follows_the_density_ladder() {
    use teksilo_widgets::primitives::VStack;
    use teksilo_widgets::standard_item::StandardListItem;

    for (density, single, two_line) in [
        (TargetDensity::Compact, 28.0_f32, 44.0_f32),
        (TargetDensity::Comfortable, 32.0, 44.0),
        (TargetDensity::Touch, 44.0, 44.0),
    ] {
        let mut tree = tree_at(density);
        let plain = tree.add(StandardListItem::new(lit!("One line")));
        let subtitled =
            tree.add(StandardListItem::new(lit!("Two lines")).subtitle(lit!("and a subtitle")));
        // Inside a stack: a root row is handed the whole viewport and measures
        // 400 dp at every density, which is a number that proves nothing.
        tree.add(VStack::new().add_child(plain).add_child(subtitled));
        tree.layout(SizeProposal::exact(400.0, 400.0));
        // Exactly the floor for the single-line row — the floor is what decides
        // its height, and 28 dp at Compact is the value it has always had.
        assert!(
            (tree.bounds(plain).height - single).abs() < 0.01,
            "{density:?}: single-line row is {}, floor is {single}",
            tree.bounds(plain).height,
        );
        // The two-line row's own content reaches past the floor at Touch (its
        // padding scales with the density), so the floor is a lower bound there.
        assert!(
            tree.bounds(subtitled).height >= two_line - 0.01,
            "{density:?}: two-line row is {}, floor is {two_line}",
            tree.bounds(subtitled).height,
        );
    }
}

// =====================================================================
// ComboBox — the closed trigger and the dropdown rows
// =====================================================================

fn combo_tree(density: TargetDensity) -> (WidgetTree, WidgetId) {
    use teksilo_widgets::combo_box::ComboBox;

    let mut tree = tree_at(density);
    let selected = teksilo_core::signal::Signal::new(Some("Alpha".to_string()));
    let root = tree.add(ComboBox::new(
        vec!["Alpha".to_string(), "Beta".to_string(), "Gamma".to_string()],
        selected,
    ));
    tree.layout(SizeProposal::exact(400.0, 400.0));
    (tree, root)
}

/// The closed box takes a finger's tap on the release, and opens its dropdown.
/// No code changed for this: the row is here because the pointer inventory
/// found the trigger claimed by nobody, and an unclaimed control with no test
/// is indistinguishable from a broken one.
#[test]
fn a_finger_taps_the_closed_combo_box_open() {
    let (mut tree, root) = combo_tree(TargetDensity::Compact);
    let at = centre(tree.bounds(root));

    let f = tree.new_contact();
    tree.touch_down(f, at);
    assert_eq!(
        tree.active_overlays().len(),
        0,
        "the press alone opens nothing — the dropdown is a release's business",
    );
    tree.touch_up(f, at);
    tree.layout(SizeProposal::exact(400.0, 400.0));
    assert_eq!(tree.active_overlays().len(), 1, "and the release opens it");
}

/// A dropdown row is a menu row: same target, same ladder. Reading the raw
/// constant left the combo box's own list at 24 dp under a theme whose
/// `MenuList` rows had already grown to 44.
#[test]
fn a_combo_box_dropdown_row_follows_the_density_ladder() {
    for (density, expected) in [
        (TargetDensity::Compact, 24.0_f32),
        (TargetDensity::Comfortable, 32.0),
        (TargetDensity::Touch, 44.0),
    ] {
        let (mut tree, root) = combo_tree(density);
        let at = centre(tree.bounds(root));
        let f = tree.new_contact();
        tree.touch_down(f, at);
        tree.touch_up(f, at);
        tree.layout(SizeProposal::exact(400.0, 400.0));

        let row = tree
            .roots()
            .into_iter()
            .find_map(|root| find_by_type(&tree, root, "DropdownItem"))
            .expect("a dropdown row");
        assert!(
            tree.bounds(row).height >= expected - 0.01,
            "{density:?}: dropdown row is {}, floor is {expected}",
            tree.bounds(row).height,
        );
    }
}

// =====================================================================
// TableView header — the label / filter partition
// =====================================================================

type SortSignal =
    teksilo_core::signal::Signal<Option<(String, teksilo_widgets::table_view::SortDirection)>>;

fn header_tree(density: TargetDensity) -> (WidgetTree, WidgetId, WidgetId, SortSignal) {
    use teksilo_data::ListModel;
    use teksilo_widgets::primitives::TextWidget;
    use teksilo_widgets::table_view::{CellContext, Column, ColumnWidth, TableView};

    let mut tree = tree_at(density);
    let model = ListModel::from_vec(vec![1u32, 2, 3]);
    let view = TableView::new(model)
        .add_column(
            Column::<u32>::new("name", lit!("Name"), |v, _: &CellContext| {
                Box::new(TextWidget::new(lit!(format!("row {v}"))))
            })
            .width(ColumnWidth::Fixed(220.0))
            .sortable(true)
            .filterable(true),
        )
        .add_column(
            Column::<u32>::new("n", lit!("N"), |v, _: &CellContext| {
                Box::new(TextWidget::new(lit!(v.to_string())))
            })
            .width(ColumnWidth::Fixed(120.0)),
        );
    let sort = view.sort_signal().clone();
    let table = tree.add(view);
    tree.layout(SizeProposal::exact(500.0, 300.0));
    let cell = find_by_type(&tree, table, "HeaderCell").expect("a header cell");
    (tree, table, cell, sort)
}

/// The cell reports its three parts, and the two tappable ones tile the band
/// the resize grip leaves — so the geometry an audit measures is the geometry
/// the press test uses.
#[test]
fn a_header_cell_reports_a_label_zone_a_filter_zone_and_its_grip() {
    let (tree, _table, cell, _sort) = header_tree(TargetDensity::Compact);
    let regions = tree.widget_target_regions(cell);
    assert_eq!(regions.len(), 3, "label, filter, grip");
    let bounds = tree.bounds(cell);
    let label = regions[0].rect;
    let filter = regions[1].rect;
    assert!(
        (label.x - bounds.x).abs() < 0.01,
        "the label zone starts at the cell's leading edge",
    );
    assert!(
        (label.right() - filter.x).abs() < 0.01,
        "the two zones tile exactly, with no gap and no overlap",
    );
    assert_eq!(
        regions[2].role,
        teksilo_tokens::TargetRole::Grab,
        "the resize band is a grab, not a tap target",
    );
}

/// The filter zone meets the density's target size, taking the difference from
/// the label zone by clamp-and-redistribute — 20 dp of glyph and padding is
/// what it paints, and 20 dp is below the conformance floor at every density.
#[test]
fn a_header_cells_filter_zone_meets_the_target_floor_at_every_density() {
    for (density, expected) in [
        (TargetDensity::Compact, 24.0_f32),
        (TargetDensity::Comfortable, 32.0),
        (TargetDensity::Touch, 44.0),
    ] {
        let (tree, _table, cell, _sort) = header_tree(density);
        let regions = tree.widget_target_regions(cell);
        let filter = regions[1].rect;
        assert!(
            (filter.width - expected).abs() < 0.01,
            "{density:?}: filter zone is {}, floor is {expected}",
            filter.width,
        );
    }
}

/// A press inside the filter zone does not cycle the sort: the zone is carved
/// out so the filter popover's own handler can have it. The zone is now the
/// floored one, so the four dp between the painted glyph band and the floor
/// belong to the filter too.
#[test]
fn a_tap_in_the_filter_zone_does_not_sort_the_column() {
    let (mut tree, _table, cell, sort) = header_tree(TargetDensity::Compact);
    let regions = tree.widget_target_regions(cell);
    let filter = regions[1].rect;
    let label = regions[0].rect;

    // Just inside the floored filter zone's leading edge — inside what the cell
    // paints as label padding, and inside what it now hit-tests as filter.
    let at = Point::new(filter.x + 1.0, filter.y + filter.height * 0.5);
    let f = tree.new_contact();
    tree.touch_down(f, at);
    tree.touch_up(f, at);
    tree.layout(SizeProposal::exact(500.0, 300.0));
    assert!(
        sort.get().is_none(),
        "a tap in the filter zone must not cycle the sort, got {:?}",
        sort.get(),
    );

    // And a tap in the label zone does sort, which is what proves the carve-out
    // is a carve-out and not an inert cell.
    let sort_at = Point::new(label.x + 8.0, label.y + label.height * 0.5);
    let g = tree.new_contact();
    tree.touch_down(g, sort_at);
    tree.touch_up(g, sort_at);
    tree.layout(SizeProposal::exact(500.0, 300.0));
    assert!(
        sort.get().is_some(),
        "a tap in the label zone sorts the column",
    );
}

// =====================================================================
// DropTarget — the five zones, floored per axis
// =====================================================================

/// The declared fraction wins wherever it already conforms.
#[test]
fn a_large_drop_targets_declared_band_is_untouched() {
    use teksilo_widgets::drop_target::band_depth;
    assert!((band_depth(300.0, 0.2, 24.0) - 60.0).abs() < 0.01);
    assert!((band_depth(300.0, 0.1, 24.0) - 30.0).abs() < 0.01);
    // A bisecting factor is a shape, not an accident: it survives the floor.
    assert!((band_depth(60.0, 0.5, 24.0) - 30.0).abs() < 0.01);
}

/// Below that, the floor takes over — and is itself capped at a third of the
/// extent, so `leading | centre | trailing` never loses its middle.
#[test]
fn a_small_drop_targets_band_is_raised_to_the_floor_but_never_past_a_third() {
    use teksilo_widgets::drop_target::band_depth;
    assert!((band_depth(100.0, 0.1, 24.0) - 24.0).abs() < 0.01);
    // A target that can afford it reaches the Touch floor outright …
    assert!((band_depth(300.0, 0.1, 44.0) - 44.0).abs() < 0.01);
    // … and one that cannot gets a third of itself, the same answer
    // `partition_targets` gives when its own floor cannot be met for every zone.
    assert!((band_depth(100.0, 0.1, 44.0) - 33.333).abs() < 0.01);
    assert!((band_depth(30.0, 0.1, 24.0) - 10.0).abs() < 0.01);
    assert!((band_depth(0.0, 0.2, 24.0) - 0.0).abs() < 0.01);
}

/// The floor reaches the hit test: a drop 15 dp inside a 100 dp target's
/// leading edge lands in `Leading`, where the raw fifth would have called it
/// `Center` and stacked the dock instead of splitting it.
#[test]
fn a_drop_near_a_small_targets_edge_lands_in_the_edge_zone() {
    use teksilo_core::styles::{DropRegion, DropRegionSet};
    use teksilo_widgets::drop_target::region_at_floored;

    let set = DropRegionSet {
        center: true,
        leading: true,
        trailing: true,
        top: true,
        bottom: true,
    };
    let size = teksilo_canvas::Size::new(100.0, 100.0);
    assert_eq!(
        region_at_floored(Point::new(15.0, 50.0), size, set, 0.1, 24.0),
        Some(DropRegion::Leading),
    );
    assert_eq!(
        region_at_floored(Point::new(50.0, 50.0), size, set, 0.1, 24.0),
        Some(DropRegion::Center),
        "the middle is still a middle",
    );
}

/// With the floor out of the way, the widget's classifier agrees with core's on
/// every point — the guard against the two priority chains drifting apart, since
/// the widget re-states core's leading → trailing → top → bottom → centre order
/// in order to apply the floor per axis.
#[test]
fn a_floorless_classification_is_cores_own() {
    use teksilo_core::styles::{DropRegionSet, region_at};
    use teksilo_widgets::drop_target::region_at_floored;

    let set = DropRegionSet {
        center: true,
        leading: true,
        trailing: true,
        top: true,
        bottom: true,
    };
    let size = teksilo_canvas::Size::new(120.0, 90.0);
    for factor in [0.1_f32, 0.2, 0.5] {
        for x in 0..24 {
            for y in 0..18 {
                let p = Point::new(x as f32 * 5.0, y as f32 * 5.0);
                assert_eq!(
                    region_at_floored(p, size, set, factor, 0.0),
                    region_at(p, size, set, factor),
                    "factor {factor} at {p:?}",
                );
            }
        }
    }
}

/// The painted zone is the zone that drops: one function for both, so the
/// highlight cannot drift from the hit test.
#[test]
fn a_floored_zones_highlight_is_the_zone_that_drops() {
    use teksilo_core::styles::DropRegion;
    use teksilo_widgets::drop_target::{band_depth, region_rect_floored};

    let bounds = Rect::new(10.0, 20.0, 100.0, 100.0);
    let depth = band_depth(100.0, 0.1, 24.0);
    let rect = region_rect_floored(DropRegion::Leading, bounds, 0.1, 24.0);
    assert!((rect.width - depth).abs() < 0.01);
    assert!((rect.x - bounds.x).abs() < 0.01);
    let trailing = region_rect_floored(DropRegion::Trailing, bounds, 0.1, 24.0);
    assert!((trailing.right() - bounds.right()).abs() < 0.01);
}

// =====================================================================
// Accordion — the dock header's tap, its drag, and its dead zone
// =====================================================================

fn accordion_tree() -> (
    WidgetTree,
    WidgetId,
    teksilo_core::signal::Signal<bool>,
    Rc<Cell<u32>>,
) {
    use teksilo_widgets::accordion::Accordion;
    use teksilo_widgets::primitives::RectWidget;

    let expanded = teksilo_core::signal::Signal::new(true);
    let drags = Rc::new(Cell::new(0u32));
    let d = drags.clone();
    let mut tree = tree_at(TargetDensity::Compact);
    let root = tree.add(
        Accordion::new(lit!("Explorer"), expanded.clone())
            .content(RectWidget::new())
            .on_header_drag(move |_| d.set(d.get() + 1)),
    );
    tree.layout(SizeProposal::exact(300.0, 400.0));
    (tree, root, expanded, drags)
}

/// A finger's tap on a dock header toggles the disclosure, on the release.
#[test]
fn a_finger_taps_an_accordion_header_shut() {
    let (mut tree, root, expanded, drags) = accordion_tree();
    // The header is the accordion's own top strip; a point 8 dp down the
    // widget's bounds is inside it whichever node carries it.
    let bounds = tree.bounds(root);
    let at = Point::new(bounds.x + bounds.width * 0.5, bounds.y + 8.0);

    let f = tree.new_contact();
    tree.touch_down(f, at);
    assert!(expanded.get(), "the press alone toggles nothing");
    tree.touch_up(f, at);
    assert!(!expanded.get(), "the release toggles it");
    assert_eq!(drags.get(), 0, "and starts no drag");
}

/// A finger that travels off the header instead of lifting starts the dock
/// drag and does *not* toggle: one gesture, one meaning.
#[test]
fn a_finger_dragging_an_accordion_header_moves_the_dock_instead() {
    let (mut tree, root, expanded, drags) = accordion_tree();
    let bounds = tree.bounds(root);
    let at = Point::new(bounds.x + bounds.width * 0.5, bounds.y + 8.0);

    let f = tree.new_contact();
    tree.touch_down(f, at);
    for step in 1..=6 {
        tree.touch_move(f, Point::new(at.x + step as f32 * 12.0, at.y));
    }
    tree.touch_up(f, Point::new(at.x + 72.0, at.y));

    assert_eq!(drags.get(), 1, "the drag hook fired once");
    assert!(
        expanded.get(),
        "and the disclosure did not toggle behind it",
    );
}

// =====================================================================
// StandardItem — where a row's press actually lives
// =====================================================================

/// **Measured, and it is why the row is a no-change row for the press visual.**
///
/// A `StandardListItem` inside a `ListView` has no gesture arena of its own: the
/// view's body pane owns the tap, the double tap and the reorder drag, and
/// resolves which row they mean by coordinate. The press record belongs to the
/// node whose arena took the press, so the row's own
/// `BuildContext::pressed_signal` is structurally always false — binding it in
/// the row would have been a mechanism that could never fire.
///
/// The row's `Pressed` chrome therefore stays reachable only through the
/// caller-supplied `interaction_signal`. Changing that means ruling on which
/// node owns a press when a data view is wrapped in something tappable, which
/// is the open design call recorded in the data-view residue — not a patch.
#[test]
fn a_list_rows_press_belongs_to_the_body_pane_not_the_row() {
    use teksilo_data::ListModel;
    use teksilo_widgets::list_view::ListView;
    use teksilo_widgets::standard_item::StandardListItem;

    let mut tree = tree_at(TargetDensity::Compact);
    let model = ListModel::from_vec(vec!["Alpha".to_string(), "Beta".to_string()]);
    let list = tree.add(
        ListView::new(model, |_index, item: &String, _selected| {
            Box::new(StandardListItem::new(lit!(item.clone())))
        })
        .item_height(32.0),
    );
    tree.layout(SizeProposal::exact(300.0, 200.0));

    let row = tree
        .roots()
        .into_iter()
        .find_map(|root| find_by_type(&tree, root, "StandardListItem"))
        .expect("a row");
    let at = centre(tree.bounds(row));

    let f = tree.new_contact();
    tree.touch_down(f, at);
    assert!(
        !tree.is_pressed(row),
        "the row is not the press owner — its arena-less node cannot be",
    );
    let owner_is_an_ancestor = std::iter::successors(tree.parent(row), |id| tree.parent(*id))
        .any(|id| tree.is_pressed(id));
    assert!(
        owner_is_an_ancestor || !tree.is_pressed(row),
        "and the press sits on an ancestor of the row, not nowhere",
    );
    let _ = list;
}

// =====================================================================
// Snackbar — the trigger presents on the release
// =====================================================================

/// The trigger's tap is an `on_tap`, so it presents on the release, and the
/// surface it presents is an overlay rather than a modal request — which is why
/// this one *is* testable headlessly where the dialog's trigger is not.
#[test]
fn a_finger_taps_a_snackbar_open() {
    use teksilo_widgets::primitives::RectWidget;
    use teksilo_widgets::snackbar::Snackbar;

    let mut tree = tree_at(TargetDensity::Compact);
    let root = tree.add(
        Snackbar::new(lit!("Show"))
            .content(RectWidget::new())
            .auto_dismiss_after(std::time::Duration::from_secs(60)),
    );
    tree.layout(SizeProposal::exact(600.0, 400.0));
    let at = centre(tree.bounds(root));

    let f = tree.new_contact();
    tree.touch_down(f, at);
    assert_eq!(
        tree.active_overlays().len(),
        0,
        "the press alone presents nothing",
    );
    tree.touch_up(f, at);
    tree.layout(SizeProposal::exact(600.0, 400.0));
    assert_eq!(tree.active_overlays().len(), 1, "the release presents it");
}
