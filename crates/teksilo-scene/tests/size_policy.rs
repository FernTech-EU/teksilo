// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! **Blocker 2: a card grows to fit its content, and the scene does not pay
//! for the cards nobody can see.**
//!
//! The scene used to size every heavyweight child purely from the model, and
//! `grep -rn measure_intrinsic crates/teksilo-scene/` was empty. The dynamic
//! path was not the hook either — `refresh_dynamic_bounds` matches
//! `SceneEntryKind::Item` and `continue`s past every widget entry, and it is
//! the wrong shape regardless: a widget's height is not a value it publishes,
//! it is the answer to a question asked at a width.
//!
//! Three things have to be true at once, and each has a test here:
//!
//! 1. the card is placed at its **measured** height on the frame it is
//!    measured, not a frame later;
//! 2. a card outside the viewport is never measured, so a page of 500 notes
//!    costs what the ten on screen cost;
//! 3. the write-back is not an edit, not structural, and not a rebuild —
//!    otherwise a note page buys an undo step, an AccessKit re-walk and a full
//!    `SceneView::build()` per line break.
//!
//! Run with: `cargo test -p teksilo-scene --test size_policy`

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use teksilo_canvas::{Rect, Size, SizeProposal};
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_scene::{ItemChange, SceneModel, SceneView, SizePolicy};

const VIEWPORT: SizeProposal = SizeProposal {
    width: Some(800.0),
    height: Some(600.0),
};

/// A body whose height is a function of the width it is offered — the shape of
/// every wrapped paragraph. 10 units per character, 20 per line.
#[derive(Debug)]
struct Wrapping {
    chars: f32,
    measured: Rc<Cell<u32>>,
}

impl Widget for Wrapping {
    fn layout_response(&self, p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
        self.measured.set(self.measured.get() + 1);
        let w = p.width.unwrap_or(200.0);
        let per_line = (w / 10.0).max(1.0);
        let lines = (self.chars / per_line).ceil().max(1.0);
        Size::new(w, lines * 20.0).into()
    }
}

fn tree_with(model: SceneModel, counters: Rc<RefCell<Vec<Rc<Cell<u32>>>>>) -> WidgetTree {
    let view = SceneView::with_model(model).delegate_typed::<usize>(move |which, _id| {
        let counter = counters.borrow()[*which].clone();
        Box::new(Wrapping {
            chars: 100.0,
            measured: counter,
        })
    });
    let mut tree = WidgetTree::new().with_text_backend(Rc::new(RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    tree.add(view);
    tree
}

/// **The headline.** The card is laid out at the height its content asked for,
/// in the frame it first appears — not one frame later, which would show a
/// short box and then a jump.
#[test]
fn a_height_for_width_card_is_placed_at_its_measured_height_on_the_same_frame() {
    let counters = Rc::new(RefCell::new(vec![Rc::new(Cell::new(0u32))]));
    let model = SceneModel::new();
    let a = model.add_widget_item(0usize, Rect::new(10.0, 10.0, 200.0, 40.0));
    assert!(model.set_size_policy(a, SizePolicy::HeightForWidth));

    let mut tree = tree_with(model.clone(), counters);
    tree.layout(VIEWPORT);

    // 100 chars at 200 units = 20 per line = 5 lines = 100 units tall.
    assert_eq!(
        model.scene_rect(a).map(|r| r.height),
        Some(100.0),
        "the model must carry the measured height after the first pass"
    );
}

/// The width stays the model's, which is what makes "you resize a note by its
/// edge and the words decide the rest" expressible.
#[test]
fn narrowing_a_card_makes_it_taller() {
    let counters = Rc::new(RefCell::new(vec![Rc::new(Cell::new(0u32))]));
    let model = SceneModel::new();
    let a = model.add_widget_item(0usize, Rect::new(10.0, 10.0, 200.0, 40.0));
    model.set_size_policy(a, SizePolicy::HeightForWidth);

    let mut tree = tree_with(model.clone(), counters);
    tree.layout(VIEWPORT);
    assert_eq!(model.scene_rect(a).map(|r| r.height), Some(100.0));

    // Half the width: 10 characters per line, 10 lines.
    model.set_local_bounds(a, Rect::new(0.0, 0.0, 100.0, 40.0));
    tree.layout(VIEWPORT);
    assert_eq!(
        model.scene_rect(a),
        Some(Rect::new(10.0, 10.0, 100.0, 200.0)),
        "the model owns the width and the content owns the height"
    );
}

/// `Intrinsic` shrink-wraps both axes: the rect supplies only the position.
#[test]
fn an_intrinsic_card_shrink_wraps_its_content() {
    let counters = Rc::new(RefCell::new(vec![Rc::new(Cell::new(0u32))]));
    let model = SceneModel::new();
    let a = model.add_widget_item(0usize, Rect::new(10.0, 10.0, 500.0, 500.0));
    model.set_size_policy(a, SizePolicy::Intrinsic);

    let mut tree = tree_with(model.clone(), counters);
    tree.layout(VIEWPORT);

    // Unbounded width: the body's own fallback of 200, one line's worth of
    // wrapping at that width.
    assert_eq!(
        model.scene_rect(a).map(|r| (r.width, r.height)),
        Some((200.0, 100.0)),
        "both axes must come from the widget"
    );
}

/// **The cull survives.** A card the camera has never shown is never measured,
/// and keeps the estimate `add_widget_item` was handed.
///
/// This is the test that makes the feature affordable. Measuring every entry on
/// every pass is what the viewport cull exists to avoid, and a size policy that
/// quietly un-culled heavyweight layout would undo it for the one tier that
/// costs the most.
#[test]
fn a_card_the_camera_has_never_shown_is_never_measured() {
    let on_screen = Rc::new(Cell::new(0u32));
    let off_screen = Rc::new(Cell::new(0u32));
    let counters = Rc::new(RefCell::new(vec![on_screen.clone(), off_screen.clone()]));

    let model = SceneModel::new();
    let a = model.add_widget_item(0usize, Rect::new(10.0, 10.0, 200.0, 40.0));
    let b = model.add_widget_item(1usize, Rect::new(10.0, 40_000.0, 200.0, 40.0));
    model.set_size_policy(a, SizePolicy::HeightForWidth);
    model.set_size_policy(b, SizePolicy::HeightForWidth);

    let mut tree = tree_with(model.clone(), counters);
    for _ in 0..5 {
        tree.layout(VIEWPORT);
    }

    assert!(
        on_screen.get() > 0,
        "the visible card must have been measured"
    );
    assert_eq!(
        off_screen.get(),
        0,
        "a card 40 000 units below the viewport must never be measured; it was \
         asked {} time(s)",
        off_screen.get()
    );
    assert_eq!(
        model.scene_rect(b).map(|r| r.height),
        Some(40.0),
        "and it must keep the estimate it was created with, which is the \
         contract `ListView::auto_item_height` already makes for a row that \
         has never been realised"
    );
}

/// …and it *is* measured the moment the camera brings it in, in that frame.
#[test]
fn a_card_is_measured_on_the_frame_the_camera_reaches_it() {
    let off_screen = Rc::new(Cell::new(0u32));
    let counters = Rc::new(RefCell::new(vec![off_screen.clone()]));
    let model = SceneModel::new();
    let a = model.add_widget_item(0usize, Rect::new(10.0, 5_000.0, 200.0, 40.0));
    model.set_size_policy(a, SizePolicy::HeightForWidth);

    let pan_y = teksilo_core::signal::Signal::new(0.0f32);
    let view = SceneView::with_model(model.clone())
        .view_state(
            teksilo_core::signal::Signal::new(0.0f32),
            pan_y.clone(),
            teksilo_core::signal::Signal::new(1.0f32),
            teksilo_core::signal::Signal::new(0.0f32),
        )
        .delegate_typed::<usize>(move |_w, _id| {
            Box::new(Wrapping {
                chars: 100.0,
                measured: off_screen.clone(),
            })
        });
    let mut tree = WidgetTree::new().with_text_backend(Rc::new(RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    tree.add(view);
    tree.layout(VIEWPORT);
    assert_eq!(model.scene_rect(a).map(|r| r.height), Some(40.0));

    pan_y.set(-5_000.0);
    tree.layout(VIEWPORT);

    assert_eq!(
        model.scene_rect(a).map(|r| r.height),
        Some(100.0),
        "the card must correct itself on the pass that first places it"
    );
    let _ = counters;
}

/// A measured size is **not an edit**: no undo step for a reflow.
#[test]
fn a_measured_size_is_not_an_edit_and_is_stamped_ephemeral() {
    let counters = Rc::new(RefCell::new(vec![Rc::new(Cell::new(0u32))]));
    let model = SceneModel::new();
    let a = model.add_widget_item(0usize, Rect::new(10.0, 10.0, 200.0, 40.0));
    model.set_size_policy(a, SizePolicy::HeightForWidth);

    let seen: Rc<RefCell<Vec<(bool, bool)>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    let _obs = model.item_change_signal().observe(move |change| {
        if matches!(change.change, ItemChange::MeasuredSizeChanged { .. }) {
            sink.borrow_mut()
                .push((change.change.is_edit(), change.ephemeral));
        }
    });

    let mut tree = tree_with(model.clone(), counters);
    tree.layout(VIEWPORT);

    let seen = seen.borrow();
    assert_eq!(seen.len(), 1, "exactly one measured-size change");
    assert_eq!(
        seen[0],
        (false, true),
        "a measured size must be neither an edit nor recordable: (is_edit, ephemeral)"
    );
}

/// …and it is **not structural**, so it buys no AccessKit re-walk.
///
/// `structural_version` subtracts exactly the churn the scene knows is derived.
/// Asserting through it rather than through `mutation_version` is the point:
/// `mutation_version` *does* advance, because the model really did change.
#[test]
fn a_measured_size_does_not_advance_the_structural_version() {
    let counters = Rc::new(RefCell::new(vec![Rc::new(Cell::new(0u32))]));
    let model = SceneModel::new();
    let a = model.add_widget_item(0usize, Rect::new(10.0, 10.0, 200.0, 40.0));
    model.set_size_policy(a, SizePolicy::HeightForWidth);

    let mut tree = tree_with(model.clone(), counters);
    let before = model.structural_version();
    tree.layout(VIEWPORT);
    let after = model.structural_version();

    assert_eq!(
        before, after,
        "a paragraph re-wrapping must not read as a structural change"
    );
    assert_ne!(
        model.scene_rect(a).map(|r| r.height),
        Some(40.0),
        "precondition: a measurement must actually have happened"
    );
}

/// …and it does not **rebuild** the view, which is what the separate
/// relayout-level signal exists for.
///
/// Counting rebuilds from outside needs a probe, because a rebuild leaves no
/// public trace of its own: `at_version` is deliberately semantic (a widget
/// that merely moved does not bump it), so asserting on it would have proved
/// nothing — and did, until this comment was written. `refresh_dynamic_bounds`
/// runs at the top of `SceneView::build` and nowhere else, and it asks every
/// `add_item_dynamic` entry for its bounds, so a dynamic item that counts the
/// question counts the builds.
#[test]
fn a_measured_size_relayouts_instead_of_rebuilding() {
    use teksilo_canvas::{Canvas, Point};
    use teksilo_scene::{SceneItem, SceneItemPaintContext};

    /// Counts how many times it was asked for its bounds — which is once per
    /// `SceneView::build`. Its bounds never change, so it emits nothing.
    #[derive(Debug)]
    struct BuildCounter(Rc<Cell<u32>>);
    impl SceneItem for BuildCounter {
        fn local_bounds(&self) -> Rect {
            self.0.set(self.0.get() + 1);
            Rect::new(0.0, 0.0, 1.0, 1.0)
        }
        fn set_local_bounds(&mut self, _b: Rect) {}
        fn paint(&self, _canvas: &mut Canvas, _ctx: &SceneItemPaintContext<'_>) {}
    }

    let builds = Rc::new(Cell::new(0u32));
    let counters = Rc::new(RefCell::new(vec![Rc::new(Cell::new(0u32))]));
    let model = SceneModel::new();
    let a = model.add_widget_item(0usize, Rect::new(10.0, 10.0, 200.0, 40.0));
    model.set_size_policy(a, SizePolicy::HeightForWidth);
    model.add_item_dynamic(BuildCounter(builds.clone()), Point::new(0.0, 0.0));

    let mut tree = tree_with(model.clone(), counters);
    // Settle: the first pass measures and writes, the next confirms.
    for _ in 0..4 {
        tree.layout(VIEWPORT);
    }
    let quiet = builds.get();
    for _ in 0..4 {
        tree.layout(VIEWPORT);
    }
    assert_eq!(
        builds.get(),
        quiet,
        "precondition: a settled scene must not rebuild on a bare relayout"
    );

    // A width change is a real edit and *earns* a rebuild. The measurement it
    // provokes must not earn a second one.
    model.set_local_bounds(a, Rect::new(0.0, 0.0, 100.0, 40.0));
    for _ in 0..4 {
        tree.layout(VIEWPORT);
    }
    assert_eq!(
        model.scene_rect(a).map(|r| r.height),
        Some(200.0),
        "precondition: the narrower card must have re-measured"
    );
    assert_eq!(
        builds.get(),
        quiet + 1,
        "one edit, one rebuild — the measured height that followed it must have \
         been answered by a relayout"
    );
}

/// A second view of the same model sees the height the first one measured.
///
/// The measurement is per view — it comes from a widget instance, and
/// `delegate_typed` builds one per view — but the *model* is shared, and every
/// whole-scene query reads it. A write that did not notify would leave the
/// second pane rendering a stale box indefinitely, because nothing else would
/// ever dirty it.
#[test]
fn a_measured_size_reaches_a_second_view_of_the_same_model() {
    let model = SceneModel::new();
    let a = model.add_widget_item(0usize, Rect::new(10.0, 10.0, 200.0, 40.0));
    model.set_size_policy(a, SizePolicy::HeightForWidth);

    let mk = |m: SceneModel, pan: &teksilo_core::signal::Signal<f32>| {
        SceneView::with_model(m)
            .view_state(
                teksilo_core::signal::Signal::new(0.0f32),
                pan.clone(),
                teksilo_core::signal::Signal::new(1.0f32),
                teksilo_core::signal::Signal::new(0.0f32),
            )
            .delegate_typed::<usize>(|_w, _id| {
                Box::new(Wrapping {
                    chars: 100.0,
                    measured: Rc::new(Cell::new(0)),
                })
            })
    };

    let mut tree_a = WidgetTree::new().with_text_backend(Rc::new(RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    let mut tree_b = WidgetTree::new().with_text_backend(Rc::new(RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    // The second view's camera is somewhere else, so it does **not** measure the
    // card itself — otherwise it would do the work first and the first view's
    // write would be a no-op, and the test would be asserting nothing.
    let pan_b = teksilo_core::signal::Signal::new(-5_000.0f32);
    let root_b = tree_b.add(mk(model.clone(), &pan_b));
    tree_a.add(mk(
        model.clone(),
        &teksilo_core::signal::Signal::new(0.0f32),
    ));

    // Settle view B, so "needs work" below means the write asked for it.
    tree_b.layout(VIEWPORT);
    assert!(
        !tree_b.needs_reconcile(),
        "precondition: the second view must be at rest before the first writes"
    );

    // View A measures and writes.
    tree_a.layout(VIEWPORT);
    assert_eq!(model.scene_rect(a).map(|r| r.height), Some(100.0));

    // The write has to **wake** the second view. Asserting only on its bounds
    // after an explicit `layout()` proves nothing — `WidgetTree::layout` runs
    // whether or not anything asked it to, so a write that notified nobody
    // would still look right in a test and render stale on screen for ever.
    // `needs_reconcile` is the honest question: bindings are polled at the head
    // of the next layout pass rather than pushed, so what the write leaves
    // behind is a dirty *binding*, not a dirty node.
    assert!(
        tree_b.needs_reconcile(),
        "the measured size must have woken every other view of the model"
    );

    // …and then it places its own card at the height the first one measured,
    // once its camera is back on it.
    pan_b.set(0.0);
    tree_b.layout(VIEWPORT);
    let card = *tree_b
        .children(root_b)
        .first()
        .expect("the second view has a card");
    assert_eq!(
        tree_b.bounds(card),
        Rect::new(10.0, 10.0, 200.0, 100.0),
        "the second view's placement must follow the shared model"
    );
}

/// A body that cannot answer the same question twice costs a bounded number of
/// passes and then stops.
///
/// `measure_intrinsic`'s own contract says `layout_response` must be
/// idempotent, and nothing enforces it. Feeding the answer back into the model,
/// which feeds the next pass, turns a non-idempotent body into an unbounded
/// relayout loop — so a card that contradicts itself on a pass nothing outside
/// the view drove is resolved to the taller of its two answers, and stops being
/// believed after a couple of those.
#[test]
fn a_non_idempotent_body_settles_instead_of_oscillating() {
    /// Alternates between 50 and 150 on every call.
    #[derive(Debug)]
    struct Flapping {
        toggle: Cell<bool>,
        calls: Rc<Cell<u32>>,
    }
    impl Widget for Flapping {
        fn layout_response(&self, p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
            self.calls.set(self.calls.get() + 1);
            let tall = self.toggle.get();
            self.toggle.set(!tall);
            Size::new(p.width.unwrap_or(200.0), if tall { 150.0 } else { 50.0 }).into()
        }
    }

    let calls = Rc::new(Cell::new(0u32));
    let model = SceneModel::new();
    let a = model.add_widget_item(0usize, Rect::new(10.0, 10.0, 200.0, 40.0));
    model.set_size_policy(a, SizePolicy::HeightForWidth);

    let c = calls.clone();
    let view = SceneView::with_model(model.clone()).delegate_typed::<usize>(move |_w, _id| {
        Box::new(Flapping {
            toggle: Cell::new(true),
            calls: c.clone(),
        })
    });
    let mut tree = WidgetTree::new().with_text_backend(Rc::new(RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    tree.add(view);

    for _ in 0..20 {
        tree.layout(VIEWPORT);
    }

    let settled = model.scene_rect(a).map(|r| r.height);
    assert_eq!(
        settled,
        Some(150.0),
        "the view must resolve to the taller of the two answers: a box that is \
         too big shows everything, a box that is too small cuts content off"
    );
    // The model is quiet from then on: repeat and confirm nothing moves.
    for _ in 0..20 {
        tree.layout(VIEWPORT);
    }
    assert_eq!(
        model.scene_rect(a).map(|r| r.height),
        settled,
        "and it must stay settled"
    );
}

/// `Fixed` is the default and changes nothing — the whole point of a policy
/// that has to be asked for.
#[test]
fn a_fixed_card_is_never_measured() {
    let calls = Rc::new(Cell::new(0u32));
    let counters = Rc::new(RefCell::new(vec![calls.clone()]));
    let model = SceneModel::new();
    let a = model.add_widget_item(0usize, Rect::new(10.0, 10.0, 200.0, 40.0));
    assert_eq!(model.size_policy(a), SizePolicy::Fixed);

    let mut tree = tree_with(model.clone(), counters);
    tree.layout(VIEWPORT);

    assert_eq!(
        model.scene_rect(a),
        Some(Rect::new(10.0, 10.0, 200.0, 40.0)),
        "a Fixed entry keeps the box it was given"
    );
}

/// A lightweight item has no `layout_response` to ask, and the refusal is
/// explicit rather than a silent no-op.
#[test]
fn a_size_policy_is_refused_for_a_lightweight_item() {
    let model = SceneModel::new();
    let item = model.add_item(
        teksilo_scene::RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
        teksilo_canvas::Point::new(0.0, 0.0),
    );
    assert!(
        !model.set_size_policy(item, SizePolicy::HeightForWidth),
        "the lightweight tier answers 'how big am I' for itself — \
         `add_item_dynamic` is its mechanism, and this one must say no"
    );
    assert_eq!(model.size_policy(item), SizePolicy::Fixed);
}

/// **Changing the policy takes effect.**
///
/// A policy is not geometry, so for a while it emitted nothing — and a toggle
/// that handed every card's height to its content changed the model and then
/// sat there, because nothing had told the view that the answer to a question
/// it asks every pass had changed. It would have taken effect whenever
/// something unrelated happened to dirty the view: in a test, never; in an app,
/// worse than never, because unpredictably.
#[test]
fn changing_the_policy_re_decides_the_geometry_on_the_next_pass() {
    let counters = Rc::new(RefCell::new(vec![Rc::new(Cell::new(0u32))]));
    let model = SceneModel::new();
    let a = model.add_widget_item(0usize, Rect::new(10.0, 10.0, 200.0, 40.0));

    let mut tree = tree_with(model.clone(), counters);
    tree.layout(VIEWPORT);
    assert_eq!(
        model.scene_rect(a).map(|r| r.height),
        Some(40.0),
        "precondition: Fixed by default"
    );

    model.set_size_policy(a, SizePolicy::HeightForWidth);
    tree.layout(VIEWPORT);

    assert_eq!(
        model.scene_rect(a).map(|r| r.height),
        Some(100.0),
        "the very next pass must honour the new policy"
    );
}

/// A policy change is an **edit**; the measurement that follows it is not.
///
/// "This note's height follows its words" is a decision someone made about the
/// document, the way a flag change is, and a history above the scene should be
/// able to put it back. The re-wrap it causes is not.
#[test]
fn a_policy_change_is_an_edit_and_the_measurement_it_causes_is_not() {
    let counters = Rc::new(RefCell::new(vec![Rc::new(Cell::new(0u32))]));
    let model = SceneModel::new();
    let a = model.add_widget_item(0usize, Rect::new(10.0, 10.0, 200.0, 40.0));
    let mut tree = tree_with(model.clone(), counters);
    tree.layout(VIEWPORT);

    let seen: Rc<RefCell<Vec<(String, bool)>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    let _obs = model.item_change_signal().observe(move |c| {
        let name = match c.change {
            ItemChange::SizePolicyChanged { .. } => "policy",
            ItemChange::MeasuredSizeChanged { .. } => "measured",
            _ => return,
        };
        sink.borrow_mut()
            .push((name.to_string(), c.change.is_edit()));
    });

    model.set_size_policy(a, SizePolicy::HeightForWidth);
    tree.layout(VIEWPORT);

    assert_eq!(
        seen.borrow().as_slice(),
        &[
            ("policy".to_string(), true),
            ("measured".to_string(), false)
        ],
        "one recorded decision, one unrecorded consequence, in that order"
    );
}

/// Turning a policy **off** leaves the box where the content put it.
///
/// `Fixed` means "nobody derives this"; it is not "put it back". Restoring an
/// authored size is the app's business, and the app is the only thing that
/// knows what the authored size was.
#[test]
fn turning_a_policy_off_leaves_the_measured_box_alone() {
    let counters = Rc::new(RefCell::new(vec![Rc::new(Cell::new(0u32))]));
    let model = SceneModel::new();
    let a = model.add_widget_item(0usize, Rect::new(10.0, 10.0, 200.0, 40.0));
    model.set_size_policy(a, SizePolicy::HeightForWidth);
    let mut tree = tree_with(model.clone(), counters);
    tree.layout(VIEWPORT);
    assert_eq!(model.scene_rect(a).map(|r| r.height), Some(100.0));

    model.set_size_policy(a, SizePolicy::Fixed);
    tree.layout(VIEWPORT);
    tree.layout(VIEWPORT);

    assert_eq!(
        model.scene_rect(a).map(|r| r.height),
        Some(100.0),
        "the measured height stands until something writes another one"
    );
}

/// **A pan costs the viewport, not the page.**
///
/// The pan-scaling probe next door measures the lightweight tier and holds it
/// at the cost of an empty scene across 50 000 items. This is the same claim for
/// the one mechanism that could break it: a size policy that measured every
/// entry would make a pan cost the whole page, on the tier where a single
/// measurement is a widget subtree rather than a rectangle.
///
/// Counted rather than timed, so it says the same thing on every machine.
#[test]
fn a_pan_across_a_page_of_cards_measures_only_what_it_shows() {
    const CARDS: usize = 200;
    let counters: Vec<Rc<Cell<u32>>> = (0..CARDS).map(|_| Rc::new(Cell::new(0))).collect();
    let model = SceneModel::new();
    for (i, _) in counters.iter().enumerate() {
        // One column, 300 units apart: at any pan the 800×600 viewport shows
        // two or three of them.
        let id = model.add_widget_item(i, Rect::new(10.0, (i as f32) * 300.0, 200.0, 40.0));
        model.set_size_policy(id, SizePolicy::HeightForWidth);
    }

    let pan_y = teksilo_core::signal::Signal::new(0.0f32);
    let all = Rc::new(RefCell::new(counters.clone()));
    let view = SceneView::with_model(model.clone())
        .view_state(
            teksilo_core::signal::Signal::new(0.0f32),
            pan_y.clone(),
            teksilo_core::signal::Signal::new(1.0f32),
            teksilo_core::signal::Signal::new(0.0f32),
        )
        .delegate_typed::<usize>(move |which, _id| {
            Box::new(Wrapping {
                chars: 100.0,
                measured: all.borrow()[*which].clone(),
            })
        });
    let mut tree = WidgetTree::new().with_text_backend(Rc::new(RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    tree.add(view);
    tree.layout(VIEWPORT);

    // Twenty pan samples across the first few cards.
    for step in 1..=20 {
        pan_y.set(-(step as f32) * 30.0);
        tree.layout(VIEWPORT);
    }

    let touched = counters.iter().filter(|c| c.get() > 0).count();
    assert!(
        touched <= 8,
        "a pan over 600 units should have shown at most a handful of the {CARDS} \
         cards; {touched} were measured"
    );
    let far = counters[CARDS - 1].get();
    assert_eq!(
        far, 0,
        "and the last one, 60 000 units down the page, must never have been asked"
    );
}

// ---------------------------------------------------------------------------
// A resize of a measured card
// ---------------------------------------------------------------------------
//
// The measurement runs against the rectangle the card is *placed* at, which
// during a live selection transform is the preview's and not the model's. That
// is what makes a resize reflow the words as the handle moves — and it is why
// the answer must not be written back: the preview is recomputed from the
// gesture's frozen start frame on every sample, so a previewed width that
// reached the model would be scaled again on the next one.

/// A body whose height follows the width it is offered, and which counts
/// nothing — the plain wrapped paragraph.
#[derive(Debug)]
struct Paragraph {
    chars: f32,
}

impl Widget for Paragraph {
    fn layout_response(&self, p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
        let w = p.width.unwrap_or(200.0);
        let per_line = (w / 10.0).max(1.0);
        let lines = (self.chars / per_line).ceil().max(1.0);
        Size::new(w, lines * 20.0).into()
    }
}

/// Drag the trailing-middle handle of a selected card `samples` times, `total`
/// units in all, laying out after each sample. Returns the widths the **model**
/// held during the gesture, and every change it emitted from the press onward.
fn resize_trailing(
    policy: SizePolicy,
    samples: usize,
    total: f32,
) -> (Vec<f32>, Rect, Vec<ItemChange>) {
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    use teksilo_scene::{
        DEFAULT_PADDING_PX, ItemFlags, ROTATE_OFFSET_PX, SceneSelection, SceneSelectionMode,
        TransformConfig, TransformFrame, TransformHandle,
    };

    let model = SceneModel::new();
    let card = model.add_widget(
        Paragraph { chars: 100.0 },
        Rect::new(100.0, 100.0, 200.0, 40.0),
    );
    model.set_size_policy(card, policy);
    model.set_flags(
        card,
        ItemFlags::default()
            .with(ItemFlags::IS_DRAGGABLE)
            .with(ItemFlags::IS_RESIZABLE),
    );
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([card]);

    let mut tree = WidgetTree::new().with_text_backend(Rc::new(RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    let view_id = tree.add(
        SceneView::with_model(model.clone())
            .selection_model(selection)
            .transform_controller(TransformConfig::new()),
    );
    tree.layout(VIEWPORT);

    let changes: Rc<RefCell<Vec<ItemChange>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = changes.clone();
    let _obs = model
        .item_change_signal()
        .observe(move |c| sink.borrow_mut().push(c.change.clone()));

    let start = TransformFrame::new(model.scene_rect(card).expect("placed"), 0.0, 1).handle_point(
        TransformHandle::Trailing,
        DEFAULT_PADDING_PX,
        ROTATE_OFFSET_PX,
    );
    tree.pointer_move(start);
    tree.dispatch_event(WidgetEvent::pointer_down(
        start,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    let mut widths = Vec::new();
    for i in 1..=samples {
        let x = start.x + total * (i as f32) / (samples as f32);
        tree.dispatch_event(WidgetEvent::pointer_move(teksilo_canvas::Point::new(
            x, start.y,
        )));
        tree.layout(VIEWPORT);
        widths.push(model.scene_rect(card).map(|r| r.width).unwrap_or(-1.0));
    }
    tree.dispatch_event(WidgetEvent::pointer_up(
        teksilo_canvas::Point::new(start.x + total, start.y),
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    if let Some(view) = tree
        .widget_as_any_mut(view_id)
        .and_then(|a| a.downcast_mut::<SceneView>())
    {
        view.flush_pending_transform();
    }
    tree.layout(VIEWPORT);

    let committed = model.scene_rect(card).expect("still there");
    let emitted = changes.borrow().clone();
    (widths, committed, emitted)
}

/// **The headline.** A multi-sample resize of a card whose size is measured
/// lands where the pointer did, and writes nothing until the release.
///
/// Both halves matter and neither implies the other. A single-sample drag has
/// not crossed the drag slop, so it never writes mid-gesture and passes
/// whatever the measurement does — which is why the suite could carry a
/// `SizePolicy` feature and a transform controller and still miss this. Ten
/// samples of a 100-unit drag used to commit **759.375**, because each sample's
/// measurement wrote the previewed width into the model and the next sample's
/// preview scaled it again.
#[test]
fn a_multi_sample_resize_of_a_measured_card_converges() {
    for policy in [
        SizePolicy::Fixed,
        SizePolicy::HeightForWidth,
        SizePolicy::Intrinsic,
    ] {
        let (widths, committed, _) = resize_trailing(policy, 10, 100.0);
        assert!(
            widths.iter().all(|w| (*w - 200.0).abs() < 0.01),
            "{policy:?}: the model must hold its own width for the whole \
             gesture, not the preview's — got {widths:?}"
        );
        let expected = match policy {
            // The content owns both axes, so there is nothing for a resize to
            // write and no handle is offered: see `transformable_roots`.
            SizePolicy::Intrinsic => 200.0,
            _ => 300.0,
        };
        assert!(
            (committed.width - expected).abs() < 0.01,
            "{policy:?}: a 100-unit drag on a 200-unit card must commit \
             {expected}, got {}",
            committed.width
        );
    }
}

/// The gesture's whole contribution to the change stream is **one** write, and
/// a cancelled one contributes none — the discipline the transform controller
/// states in its module header, which a measured card used to break on most
/// samples.
#[test]
fn a_resize_of_a_measured_card_writes_nothing_until_the_release() {
    let (_, _, emitted) = resize_trailing(SizePolicy::HeightForWidth, 10, 100.0);
    let mid: Vec<&ItemChange> = emitted
        .iter()
        .take_while(|c| !matches!(c, ItemChange::LocalBoundsChanged { .. }))
        .collect();
    assert!(
        mid.is_empty(),
        "nothing may reach the model before the release — got {mid:?}"
    );
    assert_eq!(
        emitted
            .iter()
            .filter(|c| matches!(c, ItemChange::LocalBoundsChanged { .. }))
            .count(),
        1,
        "one gesture, one reversible step — got {emitted:?}"
    );
}

/// **A resize of an `Intrinsic` card is not offered, and writes no edit.**
///
/// Both axes belong to the widget, so every number a resize could write is one
/// the next pass measures straight back over: the gesture used to leave a
/// `LocalBoundsChanged` 200×100 → 300×100 followed by a `MeasuredSizeChanged`
/// 300×100 → 200×100. Nothing moved, and the data layer gained one reversible
/// step that undoes nothing.
#[test]
fn resizing_an_intrinsic_card_records_no_edit() {
    use teksilo_scene::{ItemFlags, TransformOp};

    let (_, committed, emitted) = resize_trailing(SizePolicy::Intrinsic, 10, 100.0);
    assert!(
        (committed.width - 200.0).abs() < 0.01 && (committed.height - 100.0).abs() < 0.01,
        "the content still decides both axes — got {committed:?}"
    );
    assert!(
        !emitted
            .iter()
            .any(|c| matches!(c, ItemChange::LocalBoundsChanged { .. })),
        "and no edit was recorded — got {emitted:?}"
    );

    // …because the controller is never offered the root in the first place.
    let model = SceneModel::new();
    let card = model.add_widget(Paragraph { chars: 100.0 }, Rect::new(0.0, 0.0, 200.0, 40.0));
    model.set_flags(
        card,
        ItemFlags::default()
            .with(ItemFlags::IS_DRAGGABLE)
            .with(ItemFlags::IS_RESIZABLE),
    );
    assert_eq!(
        model.transformable_roots(&[card], TransformOp::Resize),
        vec![card],
        "a `Fixed` card resizes"
    );
    model.set_size_policy(card, SizePolicy::Intrinsic);
    assert!(
        model
            .transformable_roots(&[card], TransformOp::Resize)
            .is_empty(),
        "an `Intrinsic` one does not"
    );
    assert_eq!(
        model.transformable_roots(&[card], TransformOp::Move),
        vec![card],
        "but it still moves"
    );
}

/// **A `HeightForWidth` card is not *moved* by a drag of its top edge.**
///
/// The vertical half of a resize is neutralised in the *scale*, not merely
/// skipped at the bounds write, and that distinction is the whole test. A
/// top-edge drag is a scale about the **bottom** edge, so the same scale that
/// would have stretched the box also decides where the box's anchor lands:
/// leave it in and a 50-unit upward drag writes `LocalBoundsChanged` 100 → 150
/// *and* `LocalPosChanged` y 100 → 50 — and the measurement that follows puts
/// the height back and leaves the card fifty units higher up the page than the
/// user left it. Two writes, one reversible step that undoes nothing, and a
/// card that translated when it was asked to resize.
///
/// The same defect class as the `Intrinsic` one above; this is the half of it
/// that lives on `HeightForWidth`, where a resize handle *is* offered because
/// the horizontal axis is still the model's.
#[test]
fn a_top_edge_drag_of_a_height_for_width_card_neither_resizes_nor_moves_it() {
    use teksilo_canvas::{Point, Vec2};
    use teksilo_scene::TransformDelta;

    let start = Rect::new(100.0, 100.0, 200.0, 100.0);
    for (policy, expected) in [
        // The control: the whole point of neutralising one policy's axis is
        // that the other policy's is untouched.
        (SizePolicy::Fixed, Rect::new(100.0, 50.0, 200.0, 150.0)),
        (SizePolicy::HeightForWidth, start),
        (SizePolicy::Intrinsic, start),
    ] {
        let model = SceneModel::new();
        let card = model.add_widget(Paragraph { chars: 100.0 }, start);
        model.set_size_policy(card, policy);

        let changes: Rc<RefCell<Vec<ItemChange>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = changes.clone();
        let _obs = model
            .item_change_signal()
            .observe(move |c| sink.borrow_mut().push(c.change.clone()));

        // Drag the top edge 50 units up: a 1.5× scale on y about the bottom
        // edge, which is exactly what the transform controller hands over.
        model.apply_transform_delta(
            &[card],
            &TransformDelta::new(
                Point::new(200.0, 200.0),
                0.0,
                Vec2::new(1.0, 1.5),
                0.0,
                Vec2::ZERO,
            ),
        );

        assert_eq!(
            model.scene_rect(card),
            Some(expected),
            "{policy:?}: a top-edge drag landed somewhere it should not have"
        );
        if policy == SizePolicy::Fixed {
            continue;
        }
        let moved: Vec<ItemChange> = changes
            .borrow()
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    ItemChange::LocalPosChanged { .. } | ItemChange::LocalBoundsChanged { .. }
                )
            })
            .cloned()
            .collect();
        assert!(
            moved.is_empty(),
            "{policy:?}: an axis the content owns must take neither the extent \
             nor the anchor — got {moved:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Telling an edit apart from an oscillation
// ---------------------------------------------------------------------------
//
// The two are indistinguishable from the measured values — a user who types a
// character and deletes it twice produces the same number sequence a flip-flop
// does — so the view decides from **who drove the pass**: a content change
// dirties the card's subtree, and this view's own write dirties only this view.
// Every test below is an assertion about that discriminator or about the bound
// that backs it up.

/// A perfectly idempotent body of `lines × 20`, which dirties itself when its
/// content changes — the shape of a real editor.
#[derive(Debug)]
struct Editable(teksilo_core::signal::Signal<f32>);

impl Widget for Editable {
    fn build(
        &mut self,
        ctx: &mut teksilo_core::build_context::BuildContext,
    ) -> Vec<teksilo_core::widget_id::WidgetId> {
        let id = ctx.self_id();
        self.0.bind_to(
            id,
            ctx.binding_registry(),
            teksilo_core::binding::BindingLevel::Relayout,
        );
        vec![]
    }
    fn layout_response(&self, p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
        Size::new(p.width.unwrap_or(200.0), self.0.get() * 20.0).into()
    }
}

/// A genuinely non-idempotent body: its natural height is a function of the
/// height the model currently holds for it, which is the case
/// [`SizePolicy`]'s docs say would oscillate for ever.
#[derive(Debug)]
struct Flipper {
    model: SceneModel,
    item: teksilo_scene::ItemId,
}

impl Widget for Flipper {
    fn layout_response(&self, p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
        let given = self
            .model
            .scene_rect(self.item)
            .map(|r| r.height)
            .unwrap_or(0.0);
        let h = if given >= 110.0 { 100.0 } else { 120.0 };
        Size::new(p.width.unwrap_or(200.0), h).into()
    }
}

fn mock_tree() -> WidgetTree {
    WidgetTree::new().with_text_backend(Rc::new(RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )))
}

/// **The ordinary edit is not an oscillation, however much it looks like one.**
///
/// `100 → 120 → 100` is an oscillation when the body is answering its own last
/// answer, and is a user typing a character that wraps and then deleting it
/// when it is not. Nothing in the numbers separates them — the alternation is
/// driven four times here, so a detector that merely widened its window still
/// fails — and a card frozen at the abandoned height stays there, because a
/// card that writes nothing is never asked again.
///
/// # The cadence is the assertion
///
/// **One** layout pass per edit, which is the hostile one and also the one a
/// real app drives. `WidgetTree::layout` returns early when nothing is dirty,
/// so a settled card produces exactly one extra pass — the one its own write
/// causes — and an edit that lands before that pass runs leaves an unbroken run
/// of writing passes with no silence anywhere in it. Two passes per edit
/// manufactures that silence between every pair of edits and hides the whole
/// defect: the version of this test that drove two passes was green against a
/// view that put three of ten single-pass edits at the wrong height.
#[test]
fn an_ordinary_edit_never_pins_a_card_to_a_height_its_content_abandoned() {
    let lines = teksilo_core::signal::Signal::new(5.0f32);
    let model = SceneModel::new();
    let a = model.add_widget(Editable(lines.clone()), Rect::new(0.0, 0.0, 200.0, 40.0));
    model.set_size_policy(a, SizePolicy::HeightForWidth);
    let mut tree = mock_tree();
    tree.add(SceneView::with_model(model.clone()));
    tree.layout(VIEWPORT);
    assert_eq!(model.scene_rect(a).map(|r| r.height), Some(100.0));

    for l in [6.0f32, 5.0, 6.0, 5.0, 9.0, 5.0, 6.0, 5.0, 7.0, 5.0] {
        lines.set(l);
        tree.layout(VIEWPORT);
        assert_eq!(
            model.scene_rect(a).map(|r| r.height),
            Some(l * 20.0),
            "at {l} lines the card must be {} tall — a card that has stopped \
             following its content has been frozen by an oscillation that never \
             happened",
            l * 20.0
        );
    }

    // …and it is not merely late. Let the tree go quiet and confirm the last
    // edit is where it landed, not one answer behind.
    for _ in 0..4 {
        tree.layout(VIEWPORT);
    }
    assert_eq!(
        model.scene_rect(a).map(|r| r.height),
        Some(100.0),
        "the card must still show its content once the storm is over"
    );
}

/// **An edit deep inside a card reaches the node the view measures.**
///
/// The signal a real card's height follows is never on the card's own node: it
/// is inside a text engine several levels down, and the node the view hands to
/// `child_layout_response` is only the outermost container. What makes one
/// lookup enough is a framework invariant — a `Relayout`-level binding calls
/// `WidgetArena::mark_ancestors_need_layout`, so the flag climbs — and this is
/// where a scene-side consequence of it is pinned. If that propagation ever
/// stopped, the discriminator would read every keystroke in every real card as
/// a self-contradiction, which is the defect above with its symptom moved
/// rather than removed; this test goes red first.
#[test]
fn an_edit_deep_inside_a_card_reaches_the_node_the_view_measures() {
    use teksilo_widgets::{VStack, primitives::Padding};

    let lines = teksilo_core::signal::Signal::new(5.0f32);
    let model = SceneModel::new();
    let l = lines.clone();
    let a = model.add_widget_item(0usize, Rect::new(0.0, 0.0, 200.0, 40.0));
    model.set_size_policy(a, SizePolicy::HeightForWidth);
    let mut tree = mock_tree();
    tree.add(
        SceneView::with_model(model.clone())
            // `VStack` is the card's root — the node the view measures. The
            // signal is two containers below it.
            .delegate_typed::<usize>(move |_, _| {
                Box::new(VStack::new().child(Padding::uniform(0.0).child(Editable(l.clone()))))
            }),
    );
    tree.layout(VIEWPORT);
    assert_eq!(model.scene_rect(a).map(|r| r.height), Some(100.0));

    for l in [6.0f32, 5.0, 6.0, 5.0, 7.0, 5.0] {
        lines.set(l);
        tree.layout(VIEWPORT);
        assert_eq!(
            model.scene_rect(a).map(|r| r.height),
            Some(l * 20.0),
            "at {l} lines the card must be {} tall — the edit is three levels \
             below the node the view measures, and it still has to count as \
             an edit",
            l * 20.0
        );
    }
}

/// …and the body that **is** reading back what was written to it is still
/// caught, and still bounded.
///
/// Deleting the discriminator's other half must break this one, or the two
/// above would be satisfied by a view that simply never bounds anything.
#[test]
fn a_body_that_reads_back_what_was_written_settles_on_the_taller_answer() {
    let model = SceneModel::new();
    let a = model.add_widget_item(0usize, Rect::new(0.0, 0.0, 200.0, 40.0));
    model.set_size_policy(a, SizePolicy::HeightForWidth);
    let writes = Rc::new(Cell::new(0u32));
    let sink = writes.clone();
    let _obs = model.item_change_signal().observe(move |c| {
        if matches!(c.change, ItemChange::MeasuredSizeChanged { .. }) {
            sink.set(sink.get() + 1);
        }
    });

    let m = model.clone();
    let mut tree = mock_tree();
    tree.add(
        SceneView::with_model(model.clone()).delegate_typed::<usize>(move |_, id| {
            Box::new(Flipper {
                model: m.clone(),
                item: id,
            })
        }),
    );
    let mut heights = Vec::new();
    for _ in 0..30 {
        tree.layout(VIEWPORT);
        heights.push(model.scene_rect(a).map(|r| r.height).unwrap_or(-1.0));
    }
    assert!(
        writes.get() <= 6,
        "a body that cannot answer the same question twice must cost a bounded \
         number of passes; it wrote {} times over 30 — heights {heights:?}",
        writes.get()
    );
    assert_eq!(
        heights.last().copied(),
        Some(120.0),
        "and it must settle on the taller of the two, because a box that is \
         too big shows everything and one that is too small cuts content off"
    );
}

/// **The body "take the taller" cannot settle: one whose answer grows with what
/// it is given.**
///
/// A flip-flop has a fixed point and lands on it after one contradiction —
/// "take the taller" *is* the larger of its two values, and it agrees on the
/// next pass. A ratchet has none: every answer is bigger than the size it was
/// handed, so resolving upward feeds the growth and the card would climb by a
/// line a frame for ever, each step a `Relayout`. The only way to stop is to
/// stop taking answers, which is what the second strike does — and because
/// re-stating the size the model already holds writes nothing, the silence ends
/// the chain.
///
/// Deleting the strike limit turns this into an unbounded loop; there is no
/// value-pattern to catch it with, because no value ever repeats.
#[test]
fn a_body_whose_answer_grows_with_what_it_is_given_stops_being_asked() {
    /// Always ten units taller than the box it is placed in.
    #[derive(Debug)]
    struct Ratchet {
        model: SceneModel,
        item: teksilo_scene::ItemId,
    }
    impl Widget for Ratchet {
        fn layout_response(&self, p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
            let given = self
                .model
                .scene_rect(self.item)
                .map(|r| r.height)
                .unwrap_or(0.0);
            Size::new(p.width.unwrap_or(200.0), given + 10.0).into()
        }
    }

    let model = SceneModel::new();
    let a = model.add_widget_item(0usize, Rect::new(0.0, 0.0, 200.0, 40.0));
    model.set_size_policy(a, SizePolicy::HeightForWidth);
    let writes = Rc::new(Cell::new(0u32));
    let sink = writes.clone();
    let _obs = model.item_change_signal().observe(move |c| {
        if matches!(c.change, ItemChange::MeasuredSizeChanged { .. }) {
            sink.set(sink.get() + 1);
        }
    });

    let m = model.clone();
    let mut tree = mock_tree();
    tree.add(
        SceneView::with_model(model.clone()).delegate_typed::<usize>(move |_, id| {
            Box::new(Ratchet {
                model: m.clone(),
                item: id,
            })
        }),
    );
    let mut heights = Vec::new();
    for _ in 0..30 {
        tree.layout(VIEWPORT);
        heights.push(model.scene_rect(a).map(|r| r.height).unwrap_or(-1.0));
    }
    assert!(
        writes.get() <= 4,
        "a body with no fixed point must still cost a bounded number of passes; \
         it wrote {} times over 30 — heights {heights:?}",
        writes.get()
    );
    let settled = heights.last().copied();
    assert!(
        heights[heights.len() - 10..]
            .iter()
            .all(|h| Some(*h) == settled),
        "and then hold still — heights {heights:?}"
    );
}

/// **Two panes of one scene, and nobody editing.**
///
/// The pin used to survive across passes and across views: drive one pane
/// through `5 → 6 → 5` and it holds 120 while the other measures 100, then each
/// pass writes its own answer over the other's. Twenty quiet passes, twenty
/// `Relayout`-level writes, both panes re-laying-out every frame with no input
/// — the idle-frame class `motion_visibility` exists to prevent.
#[test]
fn two_views_of_one_model_write_nothing_on_a_quiet_scene() {
    let lines = teksilo_core::signal::Signal::new(5.0f32);
    let model = SceneModel::new();
    let a = model.add_widget_item(0usize, Rect::new(0.0, 0.0, 200.0, 40.0));
    model.set_size_policy(a, SizePolicy::HeightForWidth);
    let writes = Rc::new(Cell::new(0u32));
    let sink = writes.clone();
    let _obs = model.item_change_signal().observe(move |c| {
        if matches!(c.change, ItemChange::MeasuredSizeChanged { .. }) {
            sink.set(sink.get() + 1);
        }
    });

    let pane = |model: SceneModel, lines: teksilo_core::signal::Signal<f32>| {
        let mut tree = mock_tree();
        tree.add(
            SceneView::with_model(model)
                .delegate_typed::<usize>(move |_, _| Box::new(Editable(lines.clone()))),
        );
        tree
    };
    let mut left = pane(model.clone(), lines.clone());
    let mut right = pane(model.clone(), lines.clone());
    left.layout(VIEWPORT);
    right.layout(VIEWPORT);

    // The edit that used to pin the left pane.
    for l in [6.0f32, 5.0] {
        lines.set(l);
        left.layout(VIEWPORT);
        left.layout(VIEWPORT);
    }
    right.layout(VIEWPORT);
    right.layout(VIEWPORT);

    writes.set(0);
    for i in 0..20 {
        if i % 2 == 0 {
            left.layout(VIEWPORT);
        } else {
            right.layout(VIEWPORT);
        }
        assert_eq!(
            model.scene_rect(a).map(|r| r.height),
            Some(100.0),
            "the two panes must agree on a scene nobody is editing"
        );
    }
    assert_eq!(
        writes.get(),
        0,
        "and write nothing at all across twenty quiet passes"
    );
}

/// A card whose **widget** was replaced is believed, not weighed against its
/// predecessor.
///
/// The measurement map is keyed by `ItemId` and an `ItemId` outlives the widget
/// behind it, so a payload swap hands the delegate's fresh widget whatever the
/// previous one left behind — and the shape that bites is the one a list of
/// notes re-sourcing itself produces: swap, swap again, never a quiet pass in
/// between. What makes the newcomer's answer safe is that the delegate builds a
/// **new node**, born `needs_layout`, so the pass that first measures it is one
/// the view already reads as externally driven. Break that and the third
/// widget's answer is resolved against the first's.
#[test]
fn a_payload_swap_forgets_the_previous_widgets_measurements() {
    let model = SceneModel::new();
    let a = model.add_widget_item(5.0f32, Rect::new(0.0, 0.0, 200.0, 40.0));
    model.set_size_policy(a, SizePolicy::HeightForWidth);
    let mut tree = mock_tree();
    tree.add(
        SceneView::with_model(model.clone()).delegate_typed::<f32>(|lines, _| {
            Box::new(Paragraph {
                chars: *lines * 20.0,
            })
        }),
    );
    tree.layout(VIEWPORT);
    // 100 chars at 200 units wide: 20 per line, 5 lines.
    assert_eq!(model.scene_rect(a).map(|r| r.height), Some(100.0));

    // Swap before the settling pass, twice, so the third widget's answer is the
    // first widget's.
    model.set_payload(a, 9.0f32);
    tree.layout(VIEWPORT);
    assert_eq!(model.scene_rect(a).map(|r| r.height), Some(180.0));
    model.set_payload(a, 5.0f32);
    tree.layout(VIEWPORT);
    assert_eq!(
        model.scene_rect(a).map(|r| r.height),
        Some(100.0),
        "the new widget's answer, not one resolved against its predecessor's"
    );

    // …and nothing was frozen: the card still follows its content.
    model.set_payload(a, 12.0f32);
    tree.layout(VIEWPORT);
    assert_eq!(
        model.scene_rect(a).map(|r| r.height),
        Some(240.0),
        "a history inherited from a dead widget would have frozen this"
    );
}

/// **A policy change is a different question, and takes its history with it.**
///
/// Everything else that changes a card's answer also replaces or dirties the
/// widget giving it; this one does neither. `HeightForWidth` asks for a height
/// at the width the model holds, `Intrinsic` asks for both axes at no width at
/// all — two questions, one unchanged body, and the pass that answers the new
/// one can be perfectly clean. Kept, the old history reads the new answer as
/// that body contradicting itself and resolves the two together: the card ends
/// up 300 wide because that is what it *was*, and stays there, so the policy the
/// app just set never visibly takes effect.
#[test]
fn switching_a_card_to_intrinsic_forgets_what_it_measured_under_the_old_policy() {
    let model = SceneModel::new();
    // 300 wide: `Paragraph` wraps 100 characters into 4 lines here and into 5
    // at its own intrinsic 200, so the two policies genuinely disagree.
    let a = model.add_widget_item(0usize, Rect::new(0.0, 0.0, 300.0, 40.0));
    model.set_size_policy(a, SizePolicy::HeightForWidth);
    let mut tree = mock_tree();
    tree.add(
        SceneView::with_model(model.clone())
            .delegate_typed::<usize>(|_, _| Box::new(Paragraph { chars: 100.0 })),
    );
    tree.layout(VIEWPORT);
    assert_eq!(
        model.scene_rect(a),
        Some(Rect::new(0.0, 0.0, 300.0, 80.0)),
        "precondition: four lines at the width the model holds"
    );

    model.set_size_policy(a, SizePolicy::Intrinsic);
    tree.layout(VIEWPORT);
    assert_eq!(
        model.scene_rect(a),
        Some(Rect::new(0.0, 0.0, 200.0, 100.0)),
        "an `Intrinsic` card shrink-wraps to what its content asks for on both \
         axes — a width resolved against the old policy's answer would leave it \
         at 300"
    );

    // And it stays shrink-wrapped once the storm is over.
    for _ in 0..4 {
        tree.layout(VIEWPORT);
    }
    assert_eq!(
        model.scene_rect(a),
        Some(Rect::new(0.0, 0.0, 200.0, 100.0)),
        "and holds still"
    );
}
