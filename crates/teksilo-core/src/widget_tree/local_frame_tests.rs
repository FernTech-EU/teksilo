// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A widget's handler space, from outside a handler and from inside one:
//! `WidgetTree::window_to_local` / `local_to_window` and
//! `EventContext::to_local`, under no transform, a self transform (the shape
//! of `Scale` and `Rotate`), a content transform (the shape of `SceneView`)
//! and a singular one.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, SizeProposal, Transform2D};

use super::WidgetTree;
use crate::WidgetId;
use crate::build_context::BuildContext;
use crate::event::{EventResponse, ScrollDelta, WidgetEvent};
use crate::signal::Signal;
use crate::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use crate::widget_builder::WidgetBuilder;

/// Which transform the [`Frame`] wrapper puts over its child.
#[derive(Debug, Clone, Copy)]
enum Kind {
    None,
    /// A self transform scaling about the wrapper's centre, as `Scale` does.
    Scale(f32),
    /// A self transform rotating about the wrapper's centre, as `Rotate` does.
    Rotate(f32),
    /// A content transform, as `SceneView`'s view transform is: the wrapper
    /// keeps its bounds and its content is panned and zoomed inside them.
    Content,
    /// A self transform collapsing the horizontal axis.
    Singular,
}

/// The wrapper's content transform, shaped like a `SceneView`'s view
/// transform: pan the content by (5, −2) scene units, zoom 1.5×, then place
/// the result at the viewport's origin, which [`Offset`] fixes at (300, 200).
fn content_transform() -> Transform2D {
    Transform2D::translate(-5.0, 2.0)
        .then(&Transform2D::scale(1.5, 1.5))
        .then(&Transform2D::translate(300.0, 200.0))
}

/// Places its one child inset by 10 dp, under a transform of `kind`.
#[derive(Debug)]
struct Frame {
    kind: Kind,
    child: WidgetId,
    transform: Option<Signal<Transform2D>>,
}

impl Widget for Frame {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        match self.kind {
            Kind::None => {}
            Kind::Content => ctx.set_content_transform(id, content_transform()),
            Kind::Scale(_) | Kind::Rotate(_) | Kind::Singular => {
                let signal = ctx.signal(Transform2D::IDENTITY);
                ctx.set_transform(id, signal.clone());
                self.transform = Some(signal);
            }
        }
        vec![self.child]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        if let Some(signal) = &self.transform {
            let c = bounds.center();
            let about_centre = |t: Transform2D| {
                Transform2D::translate(-c.x, -c.y)
                    .then(&t)
                    .then(&Transform2D::translate(c.x, c.y))
            };
            signal.set(match self.kind {
                Kind::Scale(s) => about_centre(Transform2D::scale(s, s)),
                Kind::Rotate(a) => about_centre(Transform2D::rotate(a)),
                Kind::Singular => about_centre(Transform2D::scale(0.0, 1.0)),
                Kind::None | Kind::Content => Transform2D::IDENTITY,
            });
        }
        for child in children.iter_mut() {
            // Content under a content transform is placed in its own
            // coordinates (a scene's), not the viewport's.
            child.origin = match self.kind {
                Kind::Content => Point::new(10.0, 10.0),
                _ => Point::new(bounds.x + 10.0, bounds.y + 10.0),
            };
            child.size = teksilo_canvas::Size::new(bounds.width - 20.0, bounds.height - 20.0);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}

/// A leaf that fills its slot and installs nothing of its own.
#[derive(Debug)]
struct Leaf;

impl Widget for Leaf {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

/// What the leaf's handlers saw: each press's position, the same position
/// through `to_local`, and each scroll's `window_position` through it.
#[derive(Default, Debug)]
struct Seen {
    presses: Vec<(Point, Option<Point>)>,
    scrolls: Vec<Option<Point>>,
}

/// An 800×600 tree: a [`Frame`] of `kind` at (300, 200) over a recording leaf.
fn tree_with(kind: Kind) -> (WidgetTree, WidgetId, WidgetId, Rc<RefCell<Seen>>) {
    let seen = Rc::new(RefCell::new(Seen::default()));
    let mut tree = WidgetTree::new();
    let (presses, scrolls) = (seen.clone(), seen.clone());
    let leaf = tree.add(
        Leaf.on_pointer_event(move |event, ctx| {
            if let WidgetEvent::PointerDown { position, .. } = event {
                let through = ctx.pointer_position().and_then(|p| ctx.to_local(p));
                presses.borrow_mut().presses.push((*position, through));
            }
            EventResponse::Ignored
        })
        .on_scroll(move |event, ctx| {
            if let WidgetEvent::Scroll {
                window_position, ..
            } = event
            {
                let local = window_position.and_then(|p| ctx.to_local(p));
                scrolls.borrow_mut().scrolls.push(local);
            }
            EventResponse::Handled
        }),
    );
    let frame = tree.add(Frame {
        kind,
        child: leaf,
        transform: None,
    });
    let root = tree.add(Offset { child: frame });
    tree.layout(SizeProposal::exact(800.0, 600.0));
    // Layout published the wrapper's transform; lay out once more so the
    // arena's view of it is current, as a real frame's next layout would be.
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let _ = root;
    (tree, frame, leaf, seen)
}

/// Places its one child at (300, 200), 200×140: away from the window origin,
/// where a missing origin term would vanish, and with room around it, so a
/// point near the child's corner stays on the window under a 2× scale.
#[derive(Debug)]
struct Offset {
    child: WidgetId,
}

impl Widget for Offset {
    fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> {
        vec![self.child]
    }
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
    fn place_children(
        &self,
        _bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = Point::new(300.0, 200.0);
            child.size = teksilo_canvas::Size::new(200.0, 140.0);
        }
    }
    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}

fn close(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3
}

const KINDS: [Kind; 4] = [
    Kind::None,
    Kind::Scale(2.0),
    Kind::Rotate(std::f32::consts::FRAC_PI_2),
    Kind::Content,
];

/// A press sent at `local_to_window(leaf, p)` reaches the leaf's handler at
/// `p`, under every kind of transform a widget can sit under. Exactly so with
/// no transform and under a power-of-two scale, to rounding under a rotation.
#[test]
fn a_press_aimed_with_local_to_window_lands_where_it_was_aimed() {
    for kind in KINDS {
        let (mut tree, _frame, leaf, seen) = tree_with(kind);
        for p in [Point::new(12.5, 7.0), Point::new(3.0, 20.0)] {
            let at = tree.local_to_window(leaf, p);
            assert!(
                close(tree.window_to_local(leaf, at), p),
                "{kind:?}: window_to_local undoes local_to_window"
            );
            tree.pointer_down_button(at, crate::event::PointerButton::Primary);
            tree.pointer_up_button(at, crate::event::PointerButton::Primary);
            let (got, _) = *seen.borrow().presses.last().unwrap_or_else(|| {
                panic!(
                    "{kind:?}: the leaf saw no press at {at:?} for {p:?}, leaf {:?}",
                    tree.bounds(leaf)
                )
            });
            assert!(close(got, p), "{kind:?}: aimed at {p:?}, received {got:?}");
            if matches!(kind, Kind::None | Kind::Scale(_)) {
                assert_eq!(got, p, "{kind:?}: exact where the arithmetic is");
            }
        }
    }
}

/// A content-transform node's handlers receive its parent's space, not its
/// own top-left's, so its local space at the root *is* the window's — and the
/// conversion says so, where one built on the node's own transform would not.
#[test]
fn a_content_transform_node_is_local_to_its_parent_space() {
    let (tree, frame, leaf, _seen) = tree_with(Kind::Content);
    let p = Point::new(55.0, 41.0);
    assert_eq!(tree.local_to_window(frame, p), p);
    assert_eq!(tree.window_to_local(frame, p), p);
    // Its content is inside the transform: the leaf's frame includes it.
    let origin = tree.bounds(leaf).origin();
    assert!(close(
        tree.local_to_window(leaf, Point::ZERO),
        content_transform().apply_point(origin)
    ));
}

/// Inside a handler, `to_local` is the conversion the event's own position
/// went through: the same frame, the same arithmetic, the same answer.
#[test]
fn to_local_is_the_conversion_the_handlers_position_went_through() {
    for kind in KINDS {
        let (mut tree, _frame, leaf, seen) = tree_with(kind);
        let at = tree.local_to_window(leaf, Point::new(31.0, 17.5));
        tree.pointer_down_button(at, crate::event::PointerButton::Primary);
        tree.pointer_up_button(at, crate::event::PointerButton::Primary);
        let (position, through) = *seen.borrow().presses.last().expect("a press");
        assert_eq!(through, Some(position), "{kind:?}");
    }
}

/// A scroll's `window_position` stays in window space by contract; `to_local`
/// is how its handler reads it in its own.
#[test]
fn to_local_converts_a_scrolls_window_position() {
    for kind in KINDS {
        let (mut tree, _frame, leaf, seen) = tree_with(kind);
        let p = Point::new(8.0, 26.0);
        let at = tree.local_to_window(leaf, p);
        let sample = crate::pointer::ScrollSample::wheel(
            ScrollDelta::Lines { x: 0.0, y: 1.0 },
            crate::event::Modifiers::NONE,
            tree.input_now(),
        )
        .at(at);
        tree.dispatch_scroll(sample);
        let local = seen
            .borrow()
            .scrolls
            .last()
            .copied()
            .flatten()
            .expect("the leaf saw the scroll, with a position");
        assert!(close(local, p), "{kind:?}: {local:?} for {p:?}");
    }
}

/// A context made outside per-node dispatch has no widget to be local to.
#[test]
fn to_local_is_none_outside_per_node_dispatch() {
    let (mut tree, _frame, _leaf, _seen) = tree_with(Kind::None);
    let mut answer = Some(Point::ZERO);
    let mut noop = crate::window::NoopWindowOps;
    tree.run_with_event_context(&mut noop, |ctx| {
        answer = ctx.to_local(Point::new(1.0, 2.0));
    });
    assert_eq!(answer, None);
}

/// A singular transform collapses its subtree, and localisation has always
/// applied no transform rather than drop the event. The inverse does the same,
/// so the two stay each other's inverse.
#[test]
fn a_singular_transform_falls_back_to_no_transform_both_ways() {
    let (tree, _frame, leaf, _seen) = tree_with(Kind::Singular);
    let origin = tree.bounds(leaf).origin();
    let p = Point::new(12.5, 7.0);
    let at = tree.local_to_window(leaf, p);
    assert_eq!(at, Point::new(origin.x + p.x, origin.y + p.y));
    assert_eq!(tree.window_to_local(leaf, at), p);
}
