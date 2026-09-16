// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What the scene owes a pointer that goes away.
//!
//! The view re-decides three things on every hovering move — which lightweight
//! item is hovered, what the cursor says, and whether an item's tooltip is
//! armed or up. All three are *episode* state: they were opened while the
//! pointer was inside the view, and only the view can close them, because the
//! move that would have closed them is the move that never arrives.
//!
//! The departure has to come in through a route the router actually delivers.
//! `PointerEnter` / `PointerLeave` are synthesized hover transitions: the
//! preview pass refuses them outright (`try_handler_preview` returns `None` for
//! both, so an ancestor's drag guard cannot swallow a descendant's hover), and
//! the bubble matches its own `on_hover` arms before it ever reaches the
//! `on_pointer_event` catch-all. A `PointerLeave` arm inside `on_pointer_event`
//! is therefore unreachable on both passes — which is what these three measure,
//! each from a different one of the three leaks:
//!
//! * the cursor, which stays on the view's last word (`Grab`) forever if
//!   nothing withdraws it — and which *looks* correct whenever the destination
//!   happens to declare a cursor of its own, so both destinations here
//!   deliberately declare none;
//! * `on_hover`, whose `true` an item is owed a `false` for;
//! * the item tooltip, on both of its paths — the one armed and still counting
//!   down, and the one already up.
//!
//! The counterpart is `drag_cancel`, which measures the same unwind through the
//! terminal `PointerCancel`. The two arms have to agree: a departure and a
//! revocation end the same episode.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, Size, SizeProposal};
use teksilo_core::widget::{CursorIcon, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;

use crate::items::RectItem;
use crate::scene::Scene;
use crate::view::SceneView;

/// A destination that declares **no** cursor of its own.
///
/// Load-bearing: a destination that declares one writes
/// `declared_cursor_request` on its own `PointerEnter` and so repairs the
/// cursor for the scene, hiding the leak entirely. Every probe here has to land
/// somewhere silent.
#[derive(Debug)]
struct Silent;

impl Widget for Silent {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(100.0, 100.0).into()
    }
}

/// Root holding the view on the leading half and `Silent` on the trailing half,
/// so a pointer can leave the view without leaving the tree.
#[derive(Debug)]
struct SideBySide {
    view: Option<WidgetId>,
    sibling: Option<WidgetId>,
    scene: Option<Scene>,
}

impl SideBySide {
    fn new(scene: Scene) -> Self {
        Self {
            view: None,
            sibling: None,
            scene: Some(scene),
        }
    }
}

impl Widget for SideBySide {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        let scene = self.scene.take().expect("built once");
        let view = ctx.add(SceneView::new(scene));
        let sibling = ctx.add(Silent);
        self.view = Some(view);
        self.sibling = Some(sibling);
        vec![view, sibling]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let size = proposal.resolve(400.0, 300.0);
        // Measuring the children is not decoration: `SceneView` refreshes the
        // draggable-bounds snapshot its grab cursor reads from inside its own
        // `layout_response`, so a parent that only places and never measures
        // leaves the scene with no idea what is grabbable.
        let half = SizeProposal::exact(size.width / 2.0, size.height);
        for id in self.children() {
            let _ = ctx.child_size(id, half);
        }
        size.into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        let half = bounds.width / 2.0;
        if let Some(p) = children.first_mut() {
            p.origin = Point::new(bounds.x, bounds.y);
            p.size = Size::new(half, bounds.height);
        }
        if let Some(p) = children.get_mut(1) {
            p.origin = Point::new(bounds.x + half, bounds.y);
            p.size = Size::new(half, bounds.height);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.view.into_iter().chain(self.sibling).collect()
    }
}

fn tooltip_delay() -> std::time::Duration {
    teksilo_tokens::MotionTokens::default().tooltip_delay_heavy
}

/// The cursor the view raised over a draggable item has to come back down when
/// the pointer goes away — onto a silent sibling, and off the tree entirely.
///
/// `Grab` is the view's own `set_cursor` override, which by contract "outlives
/// the dispatch that set it" and is only ever re-decided by another hovering
/// move inside the view. Leaving is precisely the case where no such move
/// comes, so a departure the view never hears leaves the open hand painted over
/// whatever the pointer went on to.
#[test]
fn the_grab_cursor_comes_down_when_the_pointer_leaves_the_view() {
    let mut scene = Scene::new();
    scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 120.0, 90.0))
            .fill(teksilo_tokens::Color::RED)
            .draggable(true),
        Point::new(20.0, 20.0),
    );

    let mut tree = WidgetTree::new();
    tree.add(SideBySide::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    tree.pointer_move(Point::new(60.0, 60.0));
    assert_eq!(
        tree.current_cursor(),
        CursorIcon::Grab,
        "precondition: over a draggable item the view raises the open hand",
    );

    // Out of the view, onto a sibling with nothing to say about the cursor.
    tree.pointer_move(Point::new(300.0, 60.0));
    assert_eq!(
        tree.current_cursor(),
        CursorIcon::Default,
        "a silent destination cannot repair the cursor — the view has to",
    );

    // And back, then off the laid-out tree altogether: there is no destination
    // at all there, so nothing but the view can put the cursor down.
    tree.pointer_move(Point::new(60.0, 60.0));
    assert_eq!(tree.current_cursor(), CursorIcon::Grab, "raised again");
    tree.pointer_move(Point::new(1000.0, 1000.0));
    assert_eq!(
        tree.current_cursor(),
        CursorIcon::Default,
        "off the tree entirely, the open hand still has to come down",
    );
}

/// An item told `on_hover(true)` is owed its `false`, including when the
/// departure is the *window* boundary rather than a move to somewhere else.
///
/// This is the case the move arm structurally cannot cover: the pointer never
/// moves, so the hit is still the same item and `prev_id != new_id` is false.
/// The only thing that changed is that the pointer is gone.
#[test]
fn the_hovered_item_is_unhovered_when_the_pointer_leaves_the_window_over_it() {
    let mut scene = Scene::new();
    let item = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 120.0, 90.0)).fill(teksilo_tokens::Color::RED),
        Point::new(20.0, 20.0),
    );
    let seen: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    {
        let s = seen.clone();
        scene.handlers_mut(item).unwrap().on_hover(move |e, _ctx| {
            s.borrow_mut().push(e);
        });
    }

    let mut tree = WidgetTree::new();
    let root = tree.add(SideBySide::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view_id = tree.children(root)[0];

    tree.pointer_move(Point::new(60.0, 60.0));
    assert_eq!(
        seen.borrow().as_slice(),
        [true],
        "precondition: the item is hovered",
    );

    // The pointer leaves the window without moving — still geometrically over
    // the item, so no hover transition the move arm could read.
    let mut ops = teksilo_core::window::NoopWindowOps;
    tree.pointer_left_window(&mut ops);

    assert_eq!(
        seen.borrow().as_slice(),
        [true, false],
        "leaving the window owes the item its `false`",
    );
    assert!(
        super::view_handle(&tree, view_id)
            .hovered_item
            .get()
            .is_none(),
        "and the view stops calling it hovered",
    );
}

/// Both tooltip paths retract on a departure: the one armed and still counting
/// down, and the one already up.
///
/// The armed half is the sharper of the two — the show is queued on the tree,
/// so a tip nobody cancels appears *after* the pointer has gone, anchored to a
/// view it is no longer over.
#[test]
fn a_departure_retracts_the_item_tooltip_armed_or_shown() {
    fn fixture() -> Scene {
        let mut scene = Scene::new();
        let item = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 120.0, 90.0)).fill(teksilo_tokens::Color::RED),
            Point::new(20.0, 20.0),
        );
        scene.handlers_mut(item).unwrap().tooltip(lit!("Item tip"));
        scene
    }

    // -- armed, not yet shown --------------------------------------------
    let mut tree = WidgetTree::new();
    tree.add(SideBySide::new(fixture()));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    tree.pointer_move(Point::new(60.0, 60.0));
    tree.advance_time(tooltip_delay() / 4);
    assert!(
        tree.active_overlays().is_empty(),
        "precondition: still counting down",
    );
    tree.pointer_move(Point::new(300.0, 60.0));
    tree.advance_time(tooltip_delay() * 2);
    assert!(
        tree.active_overlays().is_empty(),
        "a tip armed inside the view must not surface after the pointer left it",
    );

    // -- already shown ---------------------------------------------------
    let mut tree = WidgetTree::new();
    tree.add(SideBySide::new(fixture()));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    tree.pointer_move(Point::new(60.0, 60.0));
    tree.advance_time(tooltip_delay() + std::time::Duration::from_millis(50));
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "precondition: the tip is up",
    );
    tree.pointer_move(Point::new(300.0, 60.0));
    assert!(
        tree.active_overlays().is_empty(),
        "and it comes down with the pointer that asked for it",
    );
}

/// The price of routing the departure through `on_hover`, stated outright so it
/// is a decision and not a surprise.
///
/// `on_hover` is the view's own node's, and the router raises it against the
/// *arena's* hover chain: crossing from one heavyweight card to another is a
/// leave of the first chain and an enter of the second, and the view is on
/// both, so it hears `false` and then `true` within the one sample. An
/// `Over`-band item that spans both cards is therefore unhovered and rehovered
/// even though the pointer never left it — `[true, false, true]` where the
/// truth is `[true]`.
///
/// It is inherent, not a choice of where to put the code. The `false` cannot be
/// deferred: the case it exists for is a pointer leaving the window, where no
/// further sample is coming and an unhover that waits for one never arrives.
/// And the leave carries nothing that could tell the two apart — the router
/// hands a hover transition no destination, the tree's pointer position is the
/// *new* one for a move and the *old* one for a window leave, and the
/// `hover_within` chain is not updated until after both dispatches. So the view
/// is told "you were left" and has to believe it.
///
/// What is bounded, and what this measures, is the damage: the item's state at
/// the end of the sample is `true` again, restored by the `PointerMove` that
/// follows the transition and re-decides the seam from the real position — so
/// no frame is ever painted with the item unhovered. The ordinary case, a scene
/// of lightweight items with no heavyweight children, has no internal chain to
/// cross and no churn at all.
#[test]
fn crossing_between_two_cards_rehovers_an_over_item_that_spans_them() {
    #[derive(Debug)]
    struct Card;
    impl Widget for Card {
        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            proposal.resolve(60.0, 60.0).into()
        }
    }

    let mut scene = Scene::new();
    scene.add_widget(Card, Rect::new(10.0, 10.0, 60.0, 60.0));
    scene.add_widget(Card, Rect::new(90.0, 10.0, 60.0, 60.0));
    let hint = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 180.0, 180.0)).fill(teksilo_tokens::Color::RED),
        Point::new(0.0, 0.0),
    );
    scene.set_layer(hint, crate::scene::SceneLayer::Over);
    scene.set_z(hint, 100.0);
    let seen: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    {
        let s = seen.clone();
        scene.handlers_mut(hint).unwrap().on_hover(move |e, _ctx| {
            s.borrow_mut().push(e);
        });
    }

    let mut tree = WidgetTree::new();
    let root = tree.add(SideBySide::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view_id = tree.children(root)[0];

    tree.pointer_move(Point::new(40.0, 40.0));
    assert_eq!(
        seen.borrow().as_slice(),
        [true],
        "the Over hint hovers while the first card holds the arena's target",
    );

    tree.pointer_move(Point::new(120.0, 40.0));
    assert_eq!(
        seen.borrow().as_slice(),
        [true, false, true],
        "crossing to the second card costs one spurious unhover — the view is \
         on both arena chains and is told it was left",
    );
    assert_eq!(
        super::view_handle(&tree, view_id).hovered_item.get(),
        Some(hint),
        "but the sample ends with the item hovered, so no frame shows otherwise",
    );
}
