// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`SceneCard`]: the gesture regime, the modes, and the accessibility shape.
//!
//! The three presses that must belong to three different owners are the part
//! that could not be written above this crate, and they are decided
//! **structurally** — by the enrolment rules the router applies at the press —
//! rather than by a recognizer race. So they are asserted by outcome: the card
//! moved, or the canvas marqueed, or neither.
//!
//! Run with: `cargo test -p teksilo-scene --test scene_card`

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, SizeProposal};
use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};
use teksilo_core::signal::Signal;
use teksilo_core::widget::Widget;
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_scene::{
    CardMode, ItemChange, ItemId, SceneCard, SceneModel, SceneSelection, SceneSelectionMode,
    SceneView, SizePolicy,
};
use teksilo_widgets::primitives::TextWidget;

const VIEWPORT: SizeProposal = SizeProposal {
    width: Some(800.0),
    height: Some(600.0),
};

/// The card's scene box. Big enough that the header strip and the body are
/// comfortably apart under the default `Card` padding.
const CARD: Rect = Rect {
    x: 100.0,
    y: 100.0,
    width: 240.0,
    height: 160.0,
};

struct Board {
    tree: WidgetTree,
    model: SceneModel,
    item: ItemId,
    selection: SceneSelection,
    mode: Signal<CardMode>,
    root: WidgetId,
    /// Every change the model emitted since the `Board` was built.
    changes: Rc<RefCell<Vec<ItemChange>>>,
    _obs: teksilo_core::signal::ObserverHandle,
}

impl Board {
    fn new() -> Self {
        Self::build(
            |c| c,
            || Box::new(teksilo_widgets::primitives::RectWidget::new().focusable(true)),
        )
    }

    fn with(tune: impl Fn(SceneCard) -> SceneCard + 'static) -> Self {
        Self::build(tune, || {
            Box::new(teksilo_widgets::primitives::RectWidget::new().focusable(true))
        })
    }

    fn with_body(body: impl Fn() -> Box<dyn Widget> + 'static) -> Self {
        Self::build(|c| c, body)
    }

    fn build(
        tune: impl Fn(SceneCard) -> SceneCard + 'static,
        body: impl Fn() -> Box<dyn Widget> + 'static,
    ) -> Self {
        let model = SceneModel::new();
        let item = model.add_widget_item(0u32, CARD);
        let selection = SceneSelection::new(SceneSelectionMode::Single);
        let mode = Signal::new(CardMode::Idle);

        let m = model.clone();
        let s = selection.clone();
        let md = mode.clone();
        let view = SceneView::with_model(model.clone())
            .selection_model(selection.clone())
            .delegate_typed::<u32>(move |_w, id| {
                Box::new(tune(
                    SceneCard::new(m.clone(), id)
                        .selection(s.clone())
                        .mode(md.clone())
                        .label(lit!("Note"))
                        .header(TextWidget::new(lit!("Title")))
                        .body(body()),
                )) as Box<dyn Widget>
            });

        let mut tree = WidgetTree::new().with_text_backend(Rc::new(RefCell::new(
            teksilo_canvas::MockTextBackend::new(),
        )));
        let root = tree.add(view);
        tree.layout(VIEWPORT);
        let _ = tree.render();

        let changes: Rc<RefCell<Vec<ItemChange>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = changes.clone();
        let obs = model
            .item_change_signal()
            .observe(move |c| sink.borrow_mut().push(c.change.clone()));

        Self {
            tree,
            model,
            item,
            selection,
            mode,
            root,
            changes,
            _obs: obs,
        }
    }

    /// A point inside the header strip, in window coordinates (which coincide
    /// with scene coordinates at the default identity camera).
    fn header_point(&self) -> Point {
        // The default `Card` padding puts the header a little inside the box;
        // a quarter of the way down the card is inside it and above the body.
        Point::new(CARD.x + CARD.width * 0.5, CARD.y + 22.0)
    }

    /// A point inside the body.
    fn body_point(&self) -> Point {
        Point::new(CARD.x + CARD.width * 0.5, CARD.y + CARD.height - 30.0)
    }

    fn press(&mut self, at: Point) {
        self.tree.dispatch_event(WidgetEvent::pointer_down(
            at,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
    }

    fn release(&mut self, at: Point) {
        self.tree.dispatch_event(WidgetEvent::pointer_up(
            at,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        self.tree.layout(VIEWPORT);
    }

    fn drag(&mut self, from: Point, steps: &[Point]) {
        self.press(from);
        for p in steps {
            self.tree.pointer_move(*p);
        }
        self.release(*steps.last().unwrap_or(&from));
    }

    fn moves(&self) -> Vec<ItemChange> {
        self.changes
            .borrow()
            .iter()
            .filter(|c| matches!(c, ItemChange::LocalPosChanged { .. }))
            .cloned()
            .collect()
    }
}

/// Dragging the header moves the card — and the whole gesture is **one** model
/// write, which is what a data-layer history needs a drag to be.
#[test]
fn a_header_drag_moves_the_card_in_one_model_write() {
    let mut b = Board::new();
    let from = b.header_point();
    b.drag(
        from,
        &[
            Point::new(from.x + 40.0, from.y + 40.0),
            Point::new(from.x + 80.0, from.y + 80.0),
            Point::new(from.x + 120.0, from.y + 120.0),
        ],
    );

    assert_eq!(
        b.model.scene_rect(b.item),
        Some(Rect::new(
            CARD.x + 120.0,
            CARD.y + 120.0,
            CARD.width,
            CARD.height
        )),
        "the card must land exactly where the pointer did"
    );
    assert_eq!(
        b.moves().len(),
        1,
        "one gesture, one reversible step — got {:?}",
        b.moves()
    );
}

/// **The slop, and the zoom.** The first few units of a drag are spent
/// crossing the recognizer's slop and are reported as a `Started` at the press
/// position, with no delta. A card that summed the recogniser's deltas would
/// trail the cursor by that distance for the rest of the gesture — visibly, and
/// for ever. And a card that read window pixels without dividing by the zoom
/// would run away from the pointer at 4×.
///
/// Both are the same test, because the fix is the same line: the drag is
/// anchored to the press and the live preview is added back, in scene units.
#[test]
fn a_header_drag_follows_the_pointer_exactly_at_4x_zoom() {
    let model = SceneModel::new();
    let item = model.add_widget_item(0u32, CARD);
    let m = model.clone();
    let view = SceneView::with_model(model.clone())
        .view_state(
            Signal::new(0.0f32),
            Signal::new(0.0f32),
            Signal::new(4.0f32),
            Signal::new(0.0f32),
        )
        .delegate_typed::<u32>(move |_w, id| {
            Box::new(
                SceneCard::new(m.clone(), id)
                    .label(lit!("Note"))
                    .header(TextWidget::new(lit!("Title")))
                    .body(teksilo_widgets::primitives::RectWidget::new()),
            ) as Box<dyn Widget>
        });
    let mut tree = WidgetTree::new().with_text_backend(Rc::new(RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    tree.add(view);
    tree.layout(VIEWPORT);
    let _ = tree.render();

    // At 4×, scene (100, 100) paints at window (400, 400) and the card is
    // 960 × 640 window px — far wider than the 800 px viewport, so the press
    // has to be near its leading edge to land on screen at all. The header
    // strip is 22 scene units down, i.e. 88 window px.
    let from = Point::new(480.0, 400.0 + 88.0);
    tree.dispatch_event(WidgetEvent::pointer_down(
        from,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    // 200 window px of travel = 50 scene units.
    for step in 1..=4 {
        tree.pointer_move(Point::new(from.x + 50.0 * step as f32, from.y));
    }
    tree.dispatch_event(WidgetEvent::pointer_up(
        Point::new(from.x + 200.0, from.y),
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.layout(VIEWPORT);

    let landed = model.scene_rect(item).expect("the card is still there");
    assert!(
        (landed.x - (CARD.x + 50.0)).abs() < 0.01,
        "200 window px at 4× zoom is 50 scene units; the card moved {} — \
         a shortfall here is the drag slop being dropped, an overshoot is the \
         zoom not being accounted for",
        landed.x - CARD.x
    );
}

/// A press on the **body** never moves the card, and never rubber-bands the
/// canvas behind it.
///
/// The second half is the one that needs a mechanism, and it is asserted two
/// ways because the weaker way passes without it. Without the card root's
/// `gesture_dead_zone`, the enrolment walk climbs past the card and enrols the
/// `SceneView`'s own `on_drag` as an ancestor, so dragging from the middle of a
/// note draws a selection rectangle over the page — and sweeps up every other
/// card it crosses.
#[test]
fn dragging_the_body_moves_nothing_and_marquees_nothing() {
    use teksilo_core::pointer::PointerId;

    let mut b = Board::new();
    // A second card, to the trailing side, for the marquee to catch if one runs.
    let other = b
        .model
        .add_widget_item(1u32, Rect::new(500.0, 100.0, 100.0, 100.0));
    b.tree.layout(VIEWPORT);

    let from = b.body_point();
    b.press(from);
    b.tree
        .pointer_move(Point::new(from.x + 200.0, from.y + 10.0));
    b.tree
        .pointer_move(Point::new(from.x + 400.0, from.y + 20.0));

    // **Mid-gesture**, which is the only moment the answer exists: after the
    // release the sequence is gone whatever it did.
    let members = b.tree.sequence_members(PointerId::MOUSE);
    let view_enrolled = members.iter().any(|(id, _, _)| *id == b.root);
    assert!(
        !view_enrolled,
        "the SceneView must not be enrolled in a gesture that started inside a \
         card; members are {members:?}"
    );

    b.release(Point::new(from.x + 400.0, from.y + 20.0));

    assert_eq!(
        b.model.scene_rect(b.item),
        Some(CARD),
        "a body drag must not move the card"
    );
    assert!(
        !b.selection.selected().contains(&other),
        "and it must not have marqueed the card it swept across; selection is \
         {:?}",
        b.selection.selected()
    );
}

/// A tap anywhere on the card selects it and brings it forward.
#[test]
fn a_tap_selects_the_card_and_brings_it_to_the_front() {
    let mut b = Board::new();
    let second = b
        .model
        .add_widget_item(1u32, Rect::new(500.0, 100.0, 100.0, 100.0));
    b.model.bring_to_front(second);
    b.tree.layout(VIEWPORT);

    let at = b.body_point();
    b.press(at);
    b.release(at);

    assert_eq!(b.selection.selected(), vec![b.item]);
    assert_eq!(b.mode.get(), CardMode::Selected);
    assert!(
        b.model.z(b.item).unwrap_or(0.0) > b.model.z(second).unwrap_or(0.0),
        "a tapped card must come forward"
    );
}

/// `bring_to_front` on an entry that is already strictly on top does nothing —
/// and, in particular, does not pull it **down**.
///
/// A click-to-front card asks for this on every press, so it is the one z
/// mutator that is called in a loop. Two things were wrong with an
/// unconditional `max_z + 1`: it counted the entry's own z in the maximum, so
/// repeating it marched the value up the float range until it stopped
/// separating entries at all; and once that was fixed by excluding self, the
/// unguarded form could hand a card sitting at z = 10 a new z of 1 and drop it
/// behind things it was in front of.
#[test]
fn bringing_the_front_entry_forward_again_changes_nothing() {
    let model = SceneModel::new();
    let a = model.add_widget_item(0u32, CARD);
    let b = model.add_widget_item(1u32, Rect::new(500.0, 100.0, 100.0, 100.0));
    model.set_z(a, 10.0);
    model.set_z(b, 0.0);

    let changes: Rc<RefCell<Vec<ItemChange>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = changes.clone();
    let _obs = model
        .item_change_signal()
        .observe(move |c| sink.borrow_mut().push(c.change.clone()));

    for _ in 0..5 {
        model.bring_to_front(a);
    }

    assert_eq!(
        model.z(a),
        Some(10.0),
        "an entry already in front must keep its z, not be pulled down to \
         one-above-the-rest"
    );
    assert!(
        changes.borrow().is_empty(),
        "and it must emit nothing at all; it emitted {:?}",
        changes.borrow()
    );
}

/// The other half: raising an entry that is **not** in front is one change,
/// however many times it is asked for.
#[test]
fn bringing_a_buried_entry_forward_repeatedly_is_one_change() {
    let model = SceneModel::new();
    let a = model.add_widget_item(0u32, CARD);
    let b = model.add_widget_item(1u32, Rect::new(500.0, 100.0, 100.0, 100.0));
    model.set_z(b, 5.0);

    let changes: Rc<RefCell<Vec<ItemChange>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = changes.clone();
    let _obs = model
        .item_change_signal()
        .observe(move |c| sink.borrow_mut().push(c.change.clone()));

    for _ in 0..5 {
        model.bring_to_front(a);
    }

    assert_eq!(model.z(a), Some(6.0));
    assert_eq!(
        changes.borrow().len(),
        1,
        "five presses on the same card must be one restack, not five — got {:?}",
        changes.borrow()
    );
}

/// Double-click enters edit mode and hands the keyboard to the body; `Esc`
/// takes it back to the card.
#[test]
fn double_click_edits_and_escape_comes_back_out() {
    let mut b = Board::new();
    let at = b.body_point();
    b.press(at);
    b.release(at);
    b.press(at);
    b.release(at);

    assert_eq!(b.mode.get(), CardMode::Editing);
    let focused = b.tree.focused().expect("something is focused");
    let card = *b.tree.children(b.root).first().expect("the card node");
    assert!(
        b.tree.is_descendant_of(focused, card),
        "edit mode must put the keyboard inside the card"
    );

    b.tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::Escape,
        modifiers: Modifiers::NONE,
        text: None,
    });
    b.tree.layout(VIEWPORT);

    assert_eq!(b.mode.get(), CardMode::Selected);
    assert_eq!(
        b.tree.focused(),
        Some(card),
        "Esc must leave the keyboard on the card, so Tab continues from the \
         object rather than from inside it"
    );
}

/// `Enter` on a focused card is the keyboard twin of the double-click.
#[test]
fn enter_on_a_focused_card_edits_it() {
    let mut b = Board::new();
    let card = *b.tree.children(b.root).first().expect("the card node");
    b.tree.focus(card);
    b.tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::Enter,
        modifiers: Modifiers::NONE,
        text: None,
    });
    b.tree.layout(VIEWPORT);

    assert_eq!(b.mode.get(), CardMode::Editing);
}

/// Focus leaving the card's subtree is the commit.
#[test]
fn focus_leaving_the_subtree_leaves_edit_mode() {
    let mut b = Board::new();
    let card = *b.tree.children(b.root).first().expect("the card node");
    b.tree.focus(card);
    b.tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::Enter,
        modifiers: Modifiers::NONE,
        text: None,
    });
    b.tree.layout(VIEWPORT);
    assert_eq!(b.mode.get(), CardMode::Editing);

    // Somewhere else entirely.
    b.tree.focus(b.root);
    b.tree.layout(VIEWPORT);

    assert_eq!(
        b.mode.get(),
        CardMode::Selected,
        "the card must notice the keyboard left it"
    );
}

/// The card contributes **one** tab stop of its own, and does not manage the
/// body's.
///
/// `tab_stops_within` is the only thing that proves reachability —
/// `WidgetTree::focus` does not check `focusable`.
///
/// A card with a focusable body is two stops, always, and that is the app's
/// decision rather than the card's: the body is mounted whether or not the card
/// is editing, and nothing short of parking it dormant takes a *composite*
/// body's inner stops out of the Tab ring (`set_tab_stop` reaches one node, not
/// a subtree). An app that wants one stop per idle note puts a `Switcher` in
/// the body — a read-only viewer and an editor, exactly one of them live — which
/// is the shape the framework already has for it.
#[test]
fn a_card_contributes_exactly_one_tab_stop_of_its_own() {
    let b = Board::with_body(|| Box::new(teksilo_widgets::primitives::RectWidget::new()));
    let card = *b.tree.children(b.root).first().expect("the card node");
    assert_eq!(
        b.tree.tab_stops_within(card),
        vec![card],
        "with a body that is not focusable, the card is the only stop"
    );

    let b = Board::new();
    let card = *b.tree.children(b.root).first().expect("the card node");
    let stops = b.tree.tab_stops_within(card);
    assert_eq!(
        stops.len(),
        2,
        "and a focusable body is a second one — the card's, then the body's, \
         in that order: {stops:?}"
    );
    assert_eq!(stops[0], card, "the object handle comes first");
}

/// The accessibility shape, asserted through the **cached** door a platform
/// adapter is handed rather than through the snapshot that bypasses it.
#[test]
fn a_card_publishes_one_named_group_with_an_edit_action() {
    let mut b = Board::new();
    let update = b.tree.sync_accessibility();

    let group = update
        .nodes
        .iter()
        .find(|(_, n)| n.role() == accesskit::Role::Group && n.label() == Some("Note"))
        .map(|(_, n)| n.clone())
        .expect("the card must publish one named Role::Group");

    assert!(
        group.supports_action(accesskit::Action::CustomAction)
            && !group.custom_actions().is_empty(),
        "assistive technology has verbs, not a pointer: the card owes it an \
         Edit action to stand in for the double-click — and the *gate*, which \
         is what an adapter reads. See \
         `the_cards_edit_action_is_advertised_and_not_merely_listed`"
    );
    assert!(
        group.supports_action(accesskit::Action::Focus),
        "and it must be focusable from AT"
    );
}

/// Selection reaches assistive technology as `selected`, and it is a
/// repaint-level change rather than a rebuild.
#[test]
fn selecting_a_card_marks_it_selected_for_assistive_technology() {
    let mut b = Board::new();
    let _ = b.tree.sync_accessibility();

    b.mode.set(CardMode::Selected);
    b.tree.layout(VIEWPORT);
    let update = b.tree.sync_accessibility();

    let group = update
        .nodes
        .iter()
        .find(|(_, n)| n.role() == accesskit::Role::Group && n.label() == Some("Note"))
        .map(|(_, n)| n.clone())
        .expect("the card is still there");
    assert_eq!(group.is_selected(), Some(true));
}

/// A card whose height follows its content, end to end through the card's own
/// builder.
#[test]
fn height_for_width_reaches_the_entry_through_the_card() {
    let b = Board::with(|c| c.height_for_width());
    assert_eq!(
        b.model.size_policy(b.item),
        SizePolicy::HeightForWidth,
        "the builder must set the policy on the entry it is bound to"
    );
}

/// A tap on a card is not stolen by a lightweight item painted underneath it.
///
/// The scene used to run two pickers wired to opposite halves of the dispatch
/// pipeline, and this is the shape that tripped it: a ruled-paper backdrop or a
/// connector line with an `on_tap`, under a page of notes, made every note
/// unclickable.
#[test]
fn a_lightweight_item_under_a_card_does_not_steal_its_tap() {
    let model = SceneModel::new();
    let backdrop = model.add_item(
        teksilo_scene::RectItem::new(Rect::new(0.0, 0.0, 2000.0, 2000.0)),
        Point::new(0.0, 0.0),
    );
    let backdrop_taps = Rc::new(std::cell::Cell::new(0u32));
    let sink = backdrop_taps.clone();
    model.with_handlers_mut(backdrop, |h| {
        h.on_tap(move |_pt, _ctx| {
            sink.set(sink.get() + 1);
        });
    });
    let item = model.add_widget_item(0u32, CARD);
    let selection = SceneSelection::new(SceneSelectionMode::Single);

    let m = model.clone();
    let s = selection.clone();
    let view = SceneView::with_model(model.clone())
        .selection_model(selection.clone())
        .delegate_typed::<u32>(move |_w, id| {
            Box::new(
                SceneCard::new(m.clone(), id)
                    .selection(s.clone())
                    .label(lit!("Note"))
                    .body(teksilo_widgets::primitives::RectWidget::new()),
            ) as Box<dyn Widget>
        });
    let mut tree = WidgetTree::new().with_text_backend(Rc::new(RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    tree.add(view);
    tree.layout(VIEWPORT);
    let _ = tree.render();

    let at = Point::new(CARD.x + CARD.width * 0.5, CARD.y + CARD.height * 0.5);
    tree.dispatch_event(WidgetEvent::pointer_down(
        at,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_up(
        at,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.layout(VIEWPORT);

    assert_eq!(
        backdrop_taps.get(),
        0,
        "the backdrop is painted under the card and must not answer for it"
    );
    assert_eq!(
        selection.selected(),
        vec![item],
        "the card must have taken its own tap"
    );
}

/// **No accessibility regression, node for node.**
///
/// The baseline is what a note page looked like before the card existed: a
/// `Panel` wrapping a `VStack` of text. `SceneCard` must not publish more than
/// that — its surface is chrome, and left alone `Card` announces a second,
/// nameless `Role::Group` *inside* the card's own named one, which a screen
/// reader reads as a container within a container.
///
/// Asserted through `sync_accessibility` — the cached door a platform adapter
/// is handed — rather than through the snapshot that bypasses the cache.
#[test]
fn a_card_publishes_no_more_nodes_than_the_hand_rolled_panel_it_replaces() {
    fn roles(mut tree: WidgetTree) -> Vec<accesskit::Role> {
        tree.layout(VIEWPORT);
        let update = tree.sync_accessibility();
        let mut roles: Vec<accesskit::Role> = update.nodes.iter().map(|(_, n)| n.role()).collect();
        roles.sort_by_key(|r| format!("{r:?}"));
        roles
    }

    fn scene_with(
        delegate: impl Fn(SceneModel, ItemId) -> Box<dyn Widget> + 'static,
    ) -> WidgetTree {
        let model = SceneModel::new();
        model.add_widget_item(0u32, CARD);
        let m = model.clone();
        let view = SceneView::with_model(model)
            .delegate_typed::<u32>(move |_w, id| delegate(m.clone(), id));
        let mut tree = WidgetTree::new().with_text_backend(Rc::new(RefCell::new(
            teksilo_canvas::MockTextBackend::new(),
        )));
        tree.add(view);
        tree
    }

    let baseline = roles(scene_with(|_m, _id| {
        Box::new(
            teksilo_widgets::Panel::new().child(
                teksilo_widgets::primitives::VStack::new()
                    .child(TextWidget::new(lit!("Title")))
                    .child(TextWidget::new(lit!("Body"))),
            ),
        )
    }));
    let carded = roles(scene_with(|m, id| {
        Box::new(
            SceneCard::new(m, id)
                .label(lit!("Note"))
                .header(TextWidget::new(lit!("Title")))
                .body(TextWidget::new(lit!("Body"))),
        )
    }));

    assert_eq!(
        carded, baseline,
        "the card must publish the same roles as the panel it replaces — one \
         container, two labels and their text runs"
    );

    // And the container it publishes is the one that carries the name, where
    // the baseline's carried none.
    let model = SceneModel::new();
    model.add_widget_item(0u32, CARD);
    let m = model.clone();
    let view = SceneView::with_model(model).delegate_typed::<u32>(move |_w, id| {
        Box::new(
            SceneCard::new(m.clone(), id)
                .label(lit!("Note"))
                .header(TextWidget::new(lit!("Title")))
                .body(TextWidget::new(lit!("Body"))),
        ) as Box<dyn Widget>
    });
    let mut tree = WidgetTree::new().with_text_backend(Rc::new(RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    tree.add(view);
    tree.layout(VIEWPORT);
    let update = tree.sync_accessibility();
    let named: Vec<_> = update
        .nodes
        .iter()
        .filter(|(_, n)| n.role() == accesskit::Role::Group)
        .map(|(_, n)| n.label().map(|l| l.to_string()))
        .collect();
    assert_eq!(
        named,
        vec![Some("Note".to_string())],
        "exactly one group, and it is named"
    );
}

// ---------------------------------------------------------------------------
// The public surface the first pass never exercised
// ---------------------------------------------------------------------------

/// `header_trailing` puts a control at the end of the header, and the header
/// still drags **around** it — the `Accordion` precedent, which is the whole
/// reason the slot is wrapped in a `DeadZone`.
///
/// Both halves are asserted, because the half that passes without the
/// mechanism is the one that would be written by accident: a press on the
/// control must not move the card *even when it jitters*, and a press beside it
/// must.
#[test]
fn the_trailing_header_slot_is_clickable_and_does_not_drag_the_card() {
    let taps = Rc::new(std::cell::Cell::new(0u32));
    let sink = taps.clone();
    let mut b = Board::with(move |c| {
        let sink = sink.clone();
        c.header_trailing(
            teksilo_widgets::primitives::FixedSize::new()
                .width(20.0)
                .height(20.0)
                .child(
                    teksilo_widgets::primitives::RectWidget::new()
                        .focusable(true)
                        .on_tap(move |_ev, _ctx| sink.set(sink.get() + 1)),
                ),
        )
    });

    // The trailing control sits at the end of the header row.
    let on_control = Point::new(CARD.x + CARD.width - 24.0, CARD.y + 22.0);
    // A real click carries a few pixels of jitter; a recogniser-timing race
    // loses this one, the structural dead zone does not.
    b.press(on_control);
    b.tree
        .pointer_move(Point::new(on_control.x + 3.0, on_control.y + 2.0));
    b.release(Point::new(on_control.x + 3.0, on_control.y + 2.0));

    assert_eq!(taps.get(), 1, "the control must take its own click");
    assert!(
        b.moves().is_empty(),
        "and the card must not have moved — got {:?}",
        b.moves()
    );

    // Beside it, the header still drags.
    let from = b.header_point();
    b.drag(from, &[Point::new(from.x + 60.0, from.y + 60.0)]);
    assert_eq!(
        b.model.scene_rect(b.item).map(|r| (r.x, r.y)),
        Some((CARD.x + 60.0, CARD.y + 60.0)),
        "the rest of the header is still the grab handle"
    );
}

/// `movable(false)` leaves the header as an ordinary strip.
#[test]
fn a_card_that_is_not_movable_does_not_move_by_its_header() {
    let mut b = Board::with(|c| c.movable(false));
    let from = b.header_point();
    b.drag(
        from,
        &[
            Point::new(from.x + 40.0, from.y + 40.0),
            Point::new(from.x + 90.0, from.y + 90.0),
        ],
    );
    assert_eq!(
        b.model.scene_rect(b.item),
        Some(CARD),
        "the card must be exactly where it started"
    );
    assert!(b.moves().is_empty(), "and have written nothing");
}

/// `on_activate` runs **before** the mode flips, which is what makes a veto
/// expressible: write the mode back inside the hook and the card honours it.
#[test]
fn on_activate_runs_first_and_can_veto_the_edit() {
    let seen = Rc::new(RefCell::new(Vec::<CardMode>::new()));

    // Observing: the hook sees the mode as it was.
    let sink = seen.clone();
    let mut b = Board::with(move |c| {
        let sink = sink.clone();
        c.on_activate(move |_ctx| sink.borrow_mut().push(CardMode::Idle))
    });
    let at = b.body_point();
    b.press(at);
    b.release(at);
    b.press(at);
    b.release(at);
    assert_eq!(seen.borrow().len(), 1, "the hook fires once per activation");
    assert_eq!(b.mode.get(), CardMode::Editing);

    // Vetoing: the hook puts the mode back and the card leaves it there.
    let mode = Signal::new(CardMode::Idle);
    let veto = mode.clone();
    let mut b = Board::with(move |c| {
        let veto = veto.clone();
        c.mode(veto.clone())
            .on_activate(move |_ctx| veto.set(CardMode::Idle))
    });
    let at = b.body_point();
    b.press(at);
    b.release(at);
    b.press(at);
    b.release(at);
    assert_eq!(
        mode.get(),
        CardMode::Idle,
        "a hook that writes the mode owns the decision"
    );
}

/// `activate_label` names the custom action assistive technology sees.
#[test]
fn activate_label_names_the_at_action() {
    let mut b = Board::with(|c| c.activate_label(lit!("Rédiger")));
    let update = b.tree.sync_accessibility();
    let group = update
        .nodes
        .iter()
        .find(|(_, n)| n.role() == accesskit::Role::Group && n.label() == Some("Note"))
        .map(|(_, n)| n.clone())
        .expect("the card's group");
    assert_eq!(
        group
            .custom_actions()
            .iter()
            .map(|a| a.description.clone())
            .collect::<Vec<_>>(),
        vec!["Rédiger".to_string()],
        "the app's label must reach the published action"
    );
}

/// The slot builders take a widget the caller already mounted, and the card
/// wires it exactly as the by-value form does.
#[test]
fn the_by_id_slots_wire_a_pre_mounted_widget() {
    let taps = Rc::new(std::cell::Cell::new(0u32));
    let sink = taps.clone();
    let model = SceneModel::new();
    let item = model.add_widget_item(0u32, CARD);
    let m = model.clone();
    let view = SceneView::with_model(model.clone()).delegate_typed::<u32>(move |_w, id| {
        let sink = sink.clone();
        Box::new(PreMounted {
            model: m.clone(),
            item: id,
            sink,
            card: None,
        }) as Box<dyn Widget>
    });
    let mut tree = WidgetTree::new().with_text_backend(Rc::new(RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    let root = tree.add(view);
    tree.layout(VIEWPORT);

    // The header still drags the card…
    let from = Point::new(CARD.x + CARD.width * 0.5, CARD.y + 22.0);
    tree.dispatch_event(WidgetEvent::pointer_down(
        from,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    for step in 1..=3 {
        tree.pointer_move(Point::new(from.x + 30.0 * step as f32, from.y));
    }
    tree.dispatch_event(WidgetEvent::pointer_up(
        Point::new(from.x + 90.0, from.y),
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.layout(VIEWPORT);
    assert_eq!(
        model.scene_rect(item).map(|r| r.x),
        Some(CARD.x + 90.0),
        "a header passed by id is still the grab handle"
    );

    // …and the trailing control passed by id still takes its own click.
    let on_control = Point::new(CARD.x + 90.0 + CARD.width - 24.0, CARD.y + 22.0);
    tree.dispatch_event(WidgetEvent::pointer_down(
        on_control,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_up(
        on_control,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    assert_eq!(taps.get(), 1, "a trailing slot passed by id is live");
    let _ = root;
}

/// A card the app gave no mode signal still has one, and it is the same handle
/// the card writes into.
#[test]
fn mode_signal_hands_back_the_cards_own_mode() {
    let model = SceneModel::new();
    model.add_widget_item(0u32, CARD);
    let observed: Rc<RefCell<Option<Signal<CardMode>>>> = Rc::new(RefCell::new(None));
    let sink = observed.clone();
    let m = model.clone();
    let view = SceneView::with_model(model).delegate_typed::<u32>(move |_w, id| {
        let card = SceneCard::new(m.clone(), id)
            .label(lit!("Note"))
            .header(TextWidget::new(lit!("Title")))
            .body(teksilo_widgets::primitives::RectWidget::new().focusable(true));
        *sink.borrow_mut() = Some(card.mode_signal());
        Box::new(card) as Box<dyn Widget>
    });
    let mut tree = WidgetTree::new().with_text_backend(Rc::new(RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    tree.add(view);
    tree.layout(VIEWPORT);

    let mode = observed.borrow().clone().expect("the card's own signal");
    assert_eq!(mode.get(), CardMode::Idle);

    let at = Point::new(CARD.x + CARD.width * 0.5, CARD.y + CARD.height - 30.0);
    tree.dispatch_event(WidgetEvent::pointer_down(
        at,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_up(
        at,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.layout(VIEWPORT);
    assert_eq!(
        mode.get(),
        CardMode::Selected,
        "the handle a card hands back is the one it writes"
    );
}

/// A composing widget that mounts the card's slots itself and passes them by
/// id — the shape `header` / `body` / `header_trailing` take a `WidgetId` for.
#[derive(Debug)]
struct PreMounted {
    model: SceneModel,
    item: ItemId,
    sink: Rc<std::cell::Cell<u32>>,
    card: Option<WidgetId>,
}

impl Widget for PreMounted {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        let header = ctx.add(TextWidget::new(lit!("Title")));
        let body = ctx.add(teksilo_widgets::primitives::RectWidget::new().focusable(true));
        let sink = self.sink.clone();
        let trailing = ctx.add(
            teksilo_widgets::primitives::FixedSize::new()
                .width(20.0)
                .height(20.0)
                .child(
                    teksilo_widgets::primitives::RectWidget::new()
                        .focusable(true)
                        .on_tap(move |_ev, _ctx| sink.set(sink.get() + 1)),
                ),
        );
        let card = ctx.add(
            SceneCard::new(self.model.clone(), self.item)
                .label(lit!("Note"))
                .header(header)
                .body(body)
                .header_trailing(trailing),
        );
        self.card = Some(card);
        vec![card]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &teksilo_core::widget::LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        self.card
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn children(&self) -> Vec<WidgetId> {
        self.card.into_iter().collect()
    }
}

// ---------------------------------------------------------------------------
// The Edit action, and the gate that makes it reachable
// ---------------------------------------------------------------------------

/// **A custom-action list is decoration without the supported-action gate.**
///
/// A platform adapter reports a node's custom actions through
/// `Action::CustomAction` being supported, not through the list being
/// non-empty (`accesskit_ios-0.2.0/src/node.rs:109`). Asserting the list alone
/// — which this file used to do — is the exact wrong gate: it passes for a card
/// whose Edit action no screen reader can invoke.
#[test]
fn the_cards_edit_action_is_advertised_and_not_merely_listed() {
    let mut b = Board::new();
    let update = b.tree.sync_accessibility();
    let group = update
        .nodes
        .iter()
        .find(|(_, n)| n.role() == accesskit::Role::Group && n.label() == Some("Note"))
        .map(|(_, n)| n.clone())
        .expect("the card's group");

    assert!(
        group.supports_action(accesskit::Action::CustomAction),
        "the list is reachable only through the gate — without it the whole \
         vector is decoration"
    );
    assert_eq!(
        group
            .custom_actions()
            .iter()
            .map(|a| a.id)
            .collect::<Vec<_>>(),
        vec![0],
        "and index 0 is what `Action::CustomAction` routes to"
    );
}

/// **A double-click puts the caret in the note — including in the body shape
/// the module docs recommend.**
///
/// The `Switcher` body buys one idle tab stop by parking the editor branch
/// dormant, and that dormancy is lifted by the `visible_when` gate in the
/// *next* layout pass — after the dispatch that flipped the mode has already
/// drained its focus request. So the two recommended shapes used to be mutually
/// exclusive: one tab stop, or focus on activation, never both.
///
/// Both halves are asserted here, because either alone passes without the
/// mechanism.
#[test]
fn double_click_puts_the_caret_in_a_switcher_body() {
    use teksilo_core::window::NoopWindowOps;
    use teksilo_widgets::primitives::Switcher;

    let model = SceneModel::new();
    model.add_widget_item(0u32, CARD);
    let mode = Signal::new(CardMode::Idle);
    let m = model.clone();
    let md = mode.clone();
    let view = SceneView::with_model(model).delegate_typed::<u32>(move |_w, id| {
        let page = md.map(|m| usize::from(*m == CardMode::Editing));
        Box::new(
            SceneCard::new(m.clone(), id)
                .mode(md.clone())
                .label(lit!("Note"))
                .header(TextWidget::new(lit!("Title")))
                .body(
                    Switcher::new(page)
                        .child(TextWidget::new(lit!("Body")))
                        .child(teksilo_widgets::primitives::RectWidget::new().focusable(true)),
                ),
        ) as Box<dyn Widget>
    });
    let mut tree = WidgetTree::new().with_text_backend(Rc::new(RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    let root = tree.add(view);
    tree.layout(VIEWPORT);
    let card = *tree.children(root).first().expect("the card node");

    assert_eq!(
        tree.tab_stops_within(card),
        vec![card],
        "the whole point of the `Switcher` shape: one stop while idle"
    );

    let at = Point::new(CARD.x + CARD.width * 0.5, CARD.y + CARD.height - 30.0);
    for _ in 0..2 {
        tree.dispatch_event(WidgetEvent::pointer_down(
            at,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        tree.dispatch_event(WidgetEvent::pointer_up(
            at,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
    }
    tree.layout(VIEWPORT);
    assert_eq!(mode.get(), CardMode::Editing);

    // The app loop drains post-mount actions after the pass that woke the
    // editor; a headless driver does it by hand.
    assert!(
        tree.has_pending_mount_actions(),
        "the card must have queued the second focus request"
    );
    tree.run_mount_actions(&mut NoopWindowOps);
    tree.layout(VIEWPORT);

    let editor = *tree
        .tab_stops_within(card)
        .last()
        .expect("the editor is reachable");
    assert_ne!(editor, card, "the editor branch is awake");
    assert_eq!(
        tree.focused(),
        Some(editor),
        "a double-click on a note must leave the keyboard in the note"
    );
}

/// The same gate is what makes the two-way `mode` contract true from **outside**
/// the card: a toolbar button that writes `Editing` has no dispatch of its own
/// to focus the body from.
#[test]
fn writing_editing_from_outside_focuses_the_body() {
    use teksilo_core::window::NoopWindowOps;

    let mut b = Board::new();
    let card = *b.tree.children(b.root).first().expect("the card node");
    let body = *b
        .tree
        .tab_stops_within(card)
        .last()
        .expect("the focusable body");
    assert_ne!(body, card);

    b.mode.set(CardMode::Editing);
    b.tree.layout(VIEWPORT);
    b.tree.run_mount_actions(&mut NoopWindowOps);

    assert_eq!(
        b.tree.focused(),
        Some(body),
        "the mode signal is documented as two-way; it has to actually be"
    );
}

/// …and it stays out of the way when the keyboard is already inside the card,
/// so an `on_activate` that placed the focus deliberately is not yanked back to
/// the body's first field.
#[test]
fn the_focus_gate_does_not_move_a_keyboard_that_is_already_inside() {
    use teksilo_core::window::NoopWindowOps;

    let mut b = Board::new();
    let card = *b.tree.children(b.root).first().expect("the card node");
    let body = *b
        .tree
        .tab_stops_within(card)
        .last()
        .expect("the focusable body");

    // Somebody else put the keyboard in the body first.
    b.tree.focus(body);
    b.tree.layout(VIEWPORT);
    b.mode.set(CardMode::Editing);
    b.tree.layout(VIEWPORT);
    b.tree.run_mount_actions(&mut NoopWindowOps);

    assert_eq!(b.tree.focused(), Some(body));
    assert!(
        !b.tree.has_pending_mount_actions(),
        "and it queued nothing to do it with"
    );
}

/// **The card's content reaches a screen reader.**
///
/// The parity test above counts nodes in the update, where a hidden subtree
/// still sits node for node. What a platform adapter exposes is the walk
/// through `accesskit_consumer::common_filter`, which drops a hidden node with
/// everything under it: while `Card`'s frame said "presentational" with
/// `set_hidden()`, this walk found `Group "Note"` and nothing inside it.
#[test]
fn a_cards_title_and_body_reach_a_screen_reader() {
    fn find<'a>(
        node: accesskit_consumer::NodeRef<'a>,
        role: accesskit::Role,
        name: &str,
    ) -> Option<accesskit_consumer::NodeRef<'a>> {
        if node.role() == role && node.label().as_deref() == Some(name) {
            return Some(node);
        }
        node.filtered_children(&accesskit_consumer::common_filter)
            .find_map(|child| find(child, role, name))
    }
    fn spoken_below(node: accesskit_consumer::NodeRef<'_>, out: &mut Vec<String>) {
        for child in node.filtered_children(&accesskit_consumer::common_filter) {
            if child.role() == accesskit::Role::Label
                && let Some(text) = child.value()
            {
                out.push(text);
            }
            spoken_below(child, out);
        }
    }

    let model = SceneModel::new();
    model.add_widget_item(0u32, CARD);
    let m = model.clone();
    let view = SceneView::with_model(model).delegate_typed::<u32>(move |_w, id| {
        Box::new(
            SceneCard::new(m.clone(), id)
                .label(lit!("Note"))
                .header(TextWidget::new(lit!("Title")))
                .body(TextWidget::new(lit!("Body"))),
        ) as Box<dyn Widget>
    });
    let mut tree = WidgetTree::new().with_text_backend(Rc::new(RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    tree.add(view);
    tree.layout(VIEWPORT);
    let consumer = accesskit_consumer::Tree::new(tree.sync_accessibility(), false);
    let Some(card) = find(consumer.state().root(), accesskit::Role::Group, "Note") else {
        panic!("the card's named group must be in the tree an adapter walks");
    };
    let mut spoken = Vec::new();
    spoken_below(card, &mut spoken);
    assert_eq!(
        spoken,
        vec!["Title".to_string(), "Body".to_string()],
        "a reader inside the card must find its title and its body"
    );
}
