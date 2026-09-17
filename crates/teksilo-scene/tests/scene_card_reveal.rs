// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! **Blocker 1: the caret inside an embedded editor moves the camera.**
//!
//! The chain was already built everywhere except its last step. A
//! `RichTextEditor` calls `EventContext::ensure_visible` on every caret move;
//! the framework's reveal walk visits every `clips_children` ancestor and
//! projects the rectangle into each one's own space as it climbs; a `SceneView`
//! clips. The view then dropped the event on the floor — `grep -rn
//! ScrollIntoView crates/teksilo-scene/src/` returned nothing — so a caret
//! typed at the bottom of a note the camera had left behind stayed there.
//!
//! These tests drive a **real** `RichTextEditor` inside a **real** `SceneCard`
//! rather than a stand-in, because the interesting part is precisely the part
//! a stand-in would hard-code: which coordinate space the rectangle arrives in
//! (scene, not window — a card's arena bounds inside a `SceneView` are its
//! scene rect, because the camera is a *content* transform) and what the
//! `applied_scroll` back-channel has to carry so an outer scroll container is
//! not asked to scroll to where the target used to be.
//!
//! Run with: `cargo test -p teksilo-scene --test scene_card_reveal`

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, SizeProposal, Vec2};
use teksilo_core::event::{EventResponse, Key, Modifiers, PointerButton, WidgetEvent};
use teksilo_core::signal::Signal;
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_scene::{SceneModel, SceneView};
use teksilo_text::text_document::TextDocument;
use teksilo_widgets::primitives::ZStack;
use teksilo_widgets::rich_text::RichTextEditor;

const VIEWPORT: SizeProposal = SizeProposal {
    width: Some(800.0),
    height: Some(600.0),
};

/// Enough lines that an arrow key has somewhere to go.
const PROSE: &str = "alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\neta\ntheta";

struct Harness {
    tree: WidgetTree,
    pan_x: Signal<f32>,
    pan_y: Signal<f32>,
    /// Every `ScrollIntoView` an outer clipping container was offered, in the
    /// space it was offered in (window).
    outer: Rc<RefCell<Vec<Rect>>>,
}

impl Harness {
    /// A scene with one card holding a real editor, inside an outer clipping
    /// container that records what the walk hands it.
    fn new(interactive: bool) -> Self {
        let doc = TextDocument::new();
        doc.set_plain_text(PROSE).unwrap();
        let model = SceneModel::new();
        model.add_widget_item(0u32, Rect::new(100.0, 100.0, 200.0, 120.0));

        let pan_x = Signal::new(0.0f32);
        let pan_y = Signal::new(0.0f32);
        let view = SceneView::with_model(model)
            .interactive(interactive)
            .view_state(
                pan_x.clone(),
                pan_y.clone(),
                Signal::new(1.0f32),
                Signal::new(0.0f32),
            )
            .delegate_typed::<u32>(move |_w, _id| Box::new(RichTextEditor::editor(doc.clone())));

        let mut tree = WidgetTree::new().with_text_backend(Rc::new(RefCell::new(
            teksilo_canvas::MockTextBackend::new(),
        )));
        let view_id = tree.add(view);
        let outer: Rc<RefCell<Vec<Rect>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = outer.clone();
        let root = tree.add(
            ZStack::new()
                .child(view_id)
                .on_scroll(move |ev, _ctx| {
                    if let WidgetEvent::ScrollIntoView { target_bounds, .. } = ev {
                        sink.borrow_mut().push(*target_bounds);
                    }
                    EventResponse::Ignored
                })
                .clips_children(true),
        );
        tree.layout(VIEWPORT);
        let _ = root;
        Self {
            tree,
            pan_x,
            pan_y,
            outer,
        }
    }

    /// Click into the editor so the caret is live. The editable node is an
    /// inner leaf of a composite, so a click is the honest way in.
    fn focus_editor(&mut self) {
        let _ = self.tree.render();
        self.tree.dispatch_event(WidgetEvent::pointer_down(
            Point::new(150.0, 150.0),
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        self.tree.layout(VIEWPORT);
        assert!(
            self.tree.focused().is_some(),
            "precondition: the click must land on the embedded editor"
        );
        self.outer.borrow_mut().clear();
    }

    fn move_caret_down(&mut self, times: usize) {
        for _ in 0..times {
            self.tree.dispatch_event(WidgetEvent::KeyDown {
                key: Key::ArrowDown,
                modifiers: Modifiers::NONE,
                text: None,
            });
            self.tree.layout(VIEWPORT);
        }
    }

    fn pan(&self) -> Vec2 {
        Vec2::new(self.pan_x.get(), self.pan_y.get())
    }
}

/// **The headline.** The camera follows a caret in a card it had left behind.
#[test]
fn typing_in_an_off_screen_note_pans_the_scene_to_the_caret() {
    let mut h = Harness::new(true);
    h.focus_editor();

    // Send the card far off to the leading side. The card stays alive because
    // the user is interacting with it — `place_children` pins whatever carries
    // a live interaction — which is itself worth knowing: were it parked, the
    // caret would not exist to chase.
    h.pan_x.set(-3000.0);
    h.tree.layout(VIEWPORT);
    assert_eq!(h.pan().x, -3000.0);

    h.move_caret_down(3);

    let after = h.pan().x;
    assert!(
        after > -400.0,
        "the caret reveal must bring the card back into the viewport; \
         pan is still {after}"
    );
    // And it lands where the card actually is, not merely somewhere: the card
    // spans scene x 100..300, so a viewport of 800 showing it has a pan
    // between -300 and 0.
    assert!(
        (-300.0..=0.0).contains(&after),
        "the reveal must be *minimal* — it should stop as soon as the caret \
         fits, not scroll to the origin. pan.x = {after}"
    );
}

/// The back-channel: an outer scroll container must not be asked to scroll to
/// where the target *was*.
///
/// With `applied_scroll` left at zero the walk hands the outer container the
/// pre-pan rectangle — window x ≈ −2867 for this scene — and the outer
/// container dutifully scrolls to it, undoing the reveal. Filling it in makes
/// the outer container see a target that is already visible and do nothing.
#[test]
fn the_reveal_reports_its_delta_so_the_outer_container_is_not_double_scrolled() {
    let mut h = Harness::new(true);
    h.focus_editor();
    h.pan_x.set(-3000.0);
    h.tree.layout(VIEWPORT);

    h.move_caret_down(3);

    let seen = h.outer.borrow();
    assert!(
        seen.is_empty(),
        "after the scene panned, the outer container should have had nothing \
         left to reveal; it was offered {seen:?}"
    );
}

/// A view the user may not drive still owes a focused descendant a reveal.
///
/// `interactive(false)` switches off *input*: the wheel, the pinch, the
/// keyboard camera. A read-only page whose embedded editor scrolls away from
/// its own caret is a bug, not a policy.
#[test]
fn a_non_interactive_view_still_reveals_a_focused_descendant() {
    let mut h = Harness::new(false);
    h.focus_editor();
    h.pan_x.set(-3000.0);
    h.tree.layout(VIEWPORT);

    h.move_caret_down(3);

    assert!(
        h.pan().x > -400.0,
        "a non-interactive view must still honour a reveal; pan is {}",
        h.pan().x
    );
}

/// The other half of registering `on_scroll` unconditionally: a wheel over a
/// non-interactive view must still reach the container around it.
///
/// `try_handler_bubble` returns `None` for a node with no handler and
/// `Some(Ignored)` for one whose handler declined, and the bubble meets them at
/// the same `unwrap_or(Ignored)` — but that is an argument, and this is the
/// measurement.
#[test]
fn a_non_interactive_view_still_lets_a_wheel_through_to_an_enclosing_scroller() {
    use teksilo_core::event::ScrollDelta;

    let model = SceneModel::new();
    model.add_widget_item(0u32, Rect::new(0.0, 0.0, 100.0, 100.0));
    let view = SceneView::with_model(model)
        .interactive(false)
        .delegate_typed::<u32>(|_w, _id| Box::new(teksilo_widgets::primitives::RectWidget::new()));

    let mut tree = WidgetTree::new().with_text_backend(Rc::new(RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    let view_id = tree.add(view);
    let wheels = Rc::new(std::cell::Cell::new(0u32));
    let sink = wheels.clone();
    let _root = tree.add(
        ZStack::new()
            .child(view_id)
            .on_scroll(move |ev, _ctx| {
                if matches!(ev, WidgetEvent::Scroll { .. }) {
                    sink.set(sink.get() + 1);
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            })
            .clips_children(true),
    );
    tree.layout(VIEWPORT);
    let _ = tree.render();

    tree.dispatch_event(WidgetEvent::scroll_at(
        ScrollDelta::Pixels { x: 0.0, y: 40.0 },
        Modifiers::NONE,
        Point::new(50.0, 50.0),
    ));

    assert_eq!(
        wheels.get(),
        1,
        "a wheel over a non-interactive SceneView must bubble to the container \
         around it, exactly as it did when the view carried no scroll handler"
    );
}

/// A reveal is a jump the user did not ask for, which is the class
/// `prefers-reduced-motion` is about. The public `SceneView::ensure_visible`
/// cannot consult the preference (it has no `EventContext`); this route can,
/// and does.
///
/// Driven by a probe that asks for `ScrollMotion::Smooth` outright, not by a
/// caret. Ordinary caret movement already asks for `Instant` — which is why the
/// first version of this test, built on one, passed with the preference ignored
/// and proved nothing. Only a typewriter pin's page jump asks for `Smooth` in
/// the shipped widgets, and standing one of those up costs more setup than it
/// buys over saying outright what is being tested.
fn smooth_reveal_harness(reduced: bool) -> (WidgetTree, Signal<f32>) {
    use teksilo_core::event::ScrollMotion;

    let model = SceneModel::new();
    model.add_widget_item(0u32, Rect::new(100.0, 100.0, 200.0, 120.0));
    // Animated handles, because the control's whole claim is that the camera
    // glides. A plain `Signal::new` cannot, which is its own test below.
    let pan_y = Signal::new_animated(0.0f32);
    let view = SceneView::with_model(model)
        .view_state(
            Signal::new_animated(0.0f32),
            pan_y.clone(),
            Signal::new(1.0f32),
            Signal::new(0.0f32),
        )
        .delegate_typed::<u32>(|_w, _id| {
            Box::new(
                teksilo_widgets::primitives::RectWidget::new()
                    .focusable(true)
                    .on_key(|_ev, ctx| {
                        // The card's own scene rect, which is the space a
                        // reveal from inside a `SceneView` is stated in.
                        ctx.ensure_visible_aligned(
                            Rect::new(100.0, 100.0, 200.0, 120.0),
                            0.5,
                            ScrollMotion::Smooth,
                        );
                        EventResponse::Handled
                    }),
            )
        });

    let mut tree = WidgetTree::new().with_text_backend(Rc::new(RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    // Before the view is added: the arm reads the preference once, when the
    // handler closure is built, exactly as the wheel-pan arm beside it has
    // always done. A mid-session toggle therefore reaches a `SceneView` on its
    // next rebuild rather than on its next reveal — `EventContext` has no
    // accessor for the preference, so reading it live would be a core change.
    tree.set_accessibility_preferences(false, reduced, 1.0);
    let root = tree.add(view);
    tree.layout(VIEWPORT);
    let _ = tree.render();

    let probe = *tree
        .tab_stops_within(root)
        .iter()
        .find(|id| **id != root)
        .expect("the probe card must be a tab stop");
    tree.focus(probe);
    // Park the card well off the bottom so the reveal has real work to do.
    pan_y.set(-3000.0);
    tree.layout(VIEWPORT);
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowDown,
        modifiers: Modifiers::NONE,
        text: None,
    });
    tree.layout(VIEWPORT);
    (tree, pan_y)
}

#[test]
fn a_smooth_reveal_glides_by_default() {
    let (_tree, pan_y) = smooth_reveal_harness(false);
    assert!(
        pan_y.animation_target().is_some(),
        "CONTROL: without the preference set a Smooth reveal must animate — \
         otherwise the reduced-motion test below distinguishes nothing. \
         pan.y = {}",
        pan_y.get()
    );
}

#[test]
fn a_smooth_reveal_snaps_under_reduced_motion() {
    let (_tree, pan_y) = smooth_reveal_harness(true);
    assert!(
        pan_y.animation_target().is_none(),
        "under reduced motion the camera must arrive rather than glide"
    );
    assert!(
        pan_y.get() > -600.0,
        "and it must still have arrived; pan.y = {}",
        pan_y.get()
    );
}

/// A reveal must not be able to bring the app down over how the app declared
/// its own camera signals.
///
/// [`SceneView::view_state`] takes any `Signal<f32>`, and a plain
/// `Signal::new` one cannot animate — `animate_to` on it panics by design.
/// Before the reveal arm existed the only thing that reached that call was an
/// explicit `ensure_visible` the app had written itself; now a descendant's
/// caret can reach it, from a keystroke, in a view whose author never called
/// anything. So the smooth branch asks rather than tells, and snaps when the
/// answer is no.
#[test]
fn a_smooth_reveal_on_a_plain_pan_signal_snaps_instead_of_panicking() {
    use teksilo_core::event::ScrollMotion;

    let model = SceneModel::new();
    model.add_widget_item(0u32, Rect::new(100.0, 100.0, 200.0, 120.0));
    // Deliberately NOT `new_animated`.
    let pan_y = Signal::new(0.0f32);
    let view = SceneView::with_model(model)
        .view_state(
            Signal::new(0.0f32),
            pan_y.clone(),
            Signal::new(1.0f32),
            Signal::new(0.0f32),
        )
        .delegate_typed::<u32>(|_w, _id| {
            Box::new(
                teksilo_widgets::primitives::RectWidget::new()
                    .focusable(true)
                    .on_key(|_ev, ctx| {
                        ctx.ensure_visible_aligned(
                            Rect::new(100.0, 100.0, 200.0, 120.0),
                            0.5,
                            ScrollMotion::Smooth,
                        );
                        EventResponse::Handled
                    }),
            )
        });

    let mut tree = WidgetTree::new().with_text_backend(Rc::new(RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    let root = tree.add(view);
    tree.layout(VIEWPORT);
    let _ = tree.render();
    let probe = *tree
        .tab_stops_within(root)
        .iter()
        .find(|id| **id != root)
        .expect("the probe card must be a tab stop");
    tree.focus(probe);
    pan_y.set(-3000.0);
    tree.layout(VIEWPORT);

    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowDown,
        modifiers: Modifiers::NONE,
        text: None,
    });
    tree.layout(VIEWPORT);

    assert!(
        pan_y.get() > -600.0,
        "the reveal must have happened, by snapping; pan.y = {}",
        pan_y.get()
    );
}
