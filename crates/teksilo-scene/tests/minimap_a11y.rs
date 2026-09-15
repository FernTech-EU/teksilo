// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `SceneMinimap` says where the viewport is, and can be driven there by
//! something other than a mouse.
//!
//! The widget's only function used to be a pointer tap. It emitted no
//! AccessKit node at all, so a keyboard user, a screen-reader user and a
//! touch user reaching for it had no route to it whatsoever — on a crate
//! whose stated differentiator is per-item accessibility.
//!
//! What is pinned here, and what reddens if each mechanism is removed:
//!
//! 1. **The node exists and is a control.** `Role::Group` with a name and a
//!    value; `tab_stops_within` reaches it. Focusing a node proves its
//!    handlers run and *nothing* about reachability —
//!    `WidgetTree::focus(id)` does not check `focusable` — so the
//!    reachability claim is made with the traversal query, not with a focus
//!    call.
//! 2. **The value tracks the viewport.** The viewport signal is bound at
//!    `AccessibilityOnly` as well as `RepaintOnly`; drop that second
//!    binding and `sync_accessibility` serves its cache, so the node keeps
//!    announcing wherever the viewport was at build time. Every value
//!    assertion below goes through a real `TreeUpdate` for that reason.
//! 3. **Three routes, one callback.** Pointer, keyboard and AT all end in
//!    `on_click`.
//! 4. **The keyboard and AT routes announce; the pointer route does not.**
//!    An arrow press on an unannotated graphic is otherwise silent, so those
//!    two routes owe an utterance. A tap does not: the user aimed at the
//!    point, and click-to-recentre is a gesture people repeat, so one
//!    utterance per click is a metronome over whatever was being read. That
//!    is the same split `HsvCanvas` makes (arrows and custom actions speak,
//!    the drag does not) and the same one the data views make (the non-drag
//!    reorder speaks, the drop does not). Silence costs nothing: the node's
//!    value is the same sentence, and it follows a tap.
//! 5. **Nothing is advertised that is not handled**, and nothing is
//!    advertised at all on a minimap with no callback — a Tab stop or an
//!    action with nothing behind it is worse than none. The gate is a
//!    default, not a wall: `.focusable(true)` still works.

use std::cell::RefCell;
use std::rc::Rc;

use accesskit::{Action, Node, NodeId, Role, TreeUpdate};
use teksilo_canvas::{Point, Rect, SizeProposal};
use teksilo_core::accessibility::widget_id_to_node_id;
use teksilo_core::event::{Key, Modifiers, WidgetEvent};
use teksilo_core::signal::Signal;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_core::window::NoopWindowOps;
use teksilo_i18n::{LocalizedString, lit};
use teksilo_scene::{MinimapReadout, SceneMinimap};
use teksilo_widgets::Padding;

/// The scene every fixture describes: a 1000 × 1000 content rect with the
/// viewport parked dead centre, one fifth of it across.
const CONTENT: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 1000.0,
    height: 1000.0,
};
const VIEWPORT: Rect = Rect {
    x: 400.0,
    y: 400.0,
    width: 200.0,
    height: 200.0,
};
const PROPOSAL: SizeProposal = SizeProposal {
    width: Some(600.0),
    height: Some(600.0),
};

/// Every scene point the minimap handed the app, in order.
type Sink = Rc<RefCell<Vec<Point>>>;

struct Fixture {
    tree: WidgetTree,
    /// The minimap itself.
    minimap: WidgetId,
    /// The `Padding` above it: the root a traversal query starts from, so
    /// "reachable" means reached *through* a parent rather than asserted of
    /// the node the test already holds.
    root: WidgetId,
    viewport: Signal<Rect>,
    got: Sink,
}

impl Fixture {
    fn build(f: impl FnOnce(SceneMinimap) -> SceneMinimap) -> Self {
        let viewport = Signal::new(VIEWPORT);
        let got: Sink = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&got);
        let pan = viewport.clone();
        let mm = f(SceneMinimap::new(CONTENT, viewport.clone())
            .size(200.0, 200.0)
            // A real consumer *applies* the move — `on_click`'s contract is
            // "centre the view on this scene point" — so the fixture centres
            // the viewport signal on it as a `SceneView` consumer would. A
            // callback that only recorded would leave every assertion about
            // what a route left behind unwritable.
            .on_click(move |p, _| {
                sink.borrow_mut().push(p);
                let v = pan.get();
                pan.set(Rect::new(
                    p.x - v.width * 0.5,
                    p.y - v.height * 0.5,
                    v.width,
                    v.height,
                ));
            }));
        let mut tree = WidgetTree::new();
        let minimap = tree.add(mm);
        let root = tree.add(Padding::uniform(20.0).child_id(minimap));
        tree.layout(PROPOSAL);
        let _ = tree.render();
        Self {
            tree,
            minimap,
            root,
            viewport,
            got,
        }
    }

    fn navigating() -> Self {
        Self::build(|mm| mm)
    }

    /// A fresh `TreeUpdate`, taken the way a platform adapter takes one.
    fn at(&mut self) -> TreeUpdate {
        self.tree.layout(PROPOSAL);
        let _ = self.tree.render();
        self.tree.sync_accessibility()
    }

    fn node(&mut self) -> Node {
        let id = widget_id_to_node_id(self.minimap);
        find(&self.at(), id)
    }

    fn value(&mut self) -> String {
        self.node()
            .value()
            .expect("the minimap announces where the viewport is")
            .to_string()
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) {
        self.tree.focus(self.minimap);
        self.tree.dispatch_event(WidgetEvent::KeyDown {
            key,
            modifiers,
            text: None,
        });
    }

    fn invoke(&mut self, action: Action) -> bool {
        let id = widget_id_to_node_id(self.minimap);
        let _ = self.tree.sync_accessibility();
        self.tree
            .dispatch_access_action(id, action, None, &mut NoopWindowOps)
    }

    /// The single scene point the routes under test produced, and a clear
    /// failure when they produced none or several.
    fn only_point(&self) -> Point {
        let got = self.got.borrow();
        assert_eq!(
            got.len(),
            1,
            "exactly one navigation request was expected, got {got:?}"
        );
        got[0]
    }

    fn last_announcement(&mut self, since: u64) -> String {
        let _ = self.at();
        let got = self.tree.announcements_since(since);
        assert!(
            !got.is_empty(),
            "a non-pointer route must say where it left the viewport — \
             a screen-reader user pressing an arrow on a graphic otherwise \
             hears nothing at all"
        );
        got.last().expect("checked non-empty").text.clone()
    }

    fn announce_seq(&self) -> u64 {
        self.tree
            .announcements_since(0)
            .last()
            .map(|a| a.seq)
            .unwrap_or(0)
    }
}

fn find(update: &TreeUpdate, id: NodeId) -> Node {
    update
        .nodes
        .iter()
        .find(|(nid, _)| *nid == id)
        .map(|(_, n)| n.clone())
        .unwrap_or_else(|| {
            panic!(
                "the minimap emits no AccessKit node at all — it is invisible \
                 to every assistive technology"
            )
        })
}

fn close(actual: Point, expected: Point, what: &str) {
    assert!(
        (actual.x - expected.x).abs() < 1e-3 && (actual.y - expected.y).abs() < 1e-3,
        "{what}: expected {expected:?}, got {actual:?}"
    );
}

/// The five actions the widget both advertises and handles.
const ROUTED_ACTIONS: [Action; 5] = [
    Action::Click,
    Action::ScrollLeft,
    Action::ScrollRight,
    Action::ScrollUp,
    Action::ScrollDown,
];

// ---------------------------------------------------------------------------
// 1. The node exists, and it is a control
// ---------------------------------------------------------------------------

#[test]
fn the_minimap_emits_a_named_group_that_says_where_the_viewport_is() {
    let mut f = Fixture::navigating();
    let node = f.node();
    assert_eq!(
        node.role(),
        Role::Group,
        "a 2-D position has no ARIA role, and a property-less GenericContainer \
         is pruned by the walker — a pruned node announces nothing"
    );
    assert_eq!(node.label(), Some("Scene minimap"));
    // Viewport centred in a 1000-unit scene, covering a fifth of each axis.
    assert_eq!(
        node.value(),
        Some("Viewport at 50% across, 50% down; showing 20% of the width and 20% of the height")
    );
}

/// `WidgetTree::focus(id)` does not check `focusable`, so focusing the node
/// would prove the handlers run and nothing at all about a keyboard user
/// being able to get there. Only the traversal query answers that.
#[test]
fn a_navigating_minimap_is_reachable_by_tab() {
    let f = Fixture::navigating();
    assert_eq!(
        f.tree.tab_stops_within(f.root),
        vec![f.minimap],
        "the minimap's only function is behind a pointer gesture, so it has to \
         be in the Tab order"
    );
}

#[test]
fn a_read_only_minimap_is_announced_but_is_not_a_dead_tab_stop() {
    let viewport = Signal::new(VIEWPORT);
    let mm = SceneMinimap::new(CONTENT, viewport).size(200.0, 200.0);
    let mut tree = WidgetTree::new();
    let minimap = tree.add(mm);
    let root = tree.add(Padding::uniform(20.0).child_id(minimap));
    tree.layout(PROPOSAL);
    let _ = tree.render();
    let update = tree.sync_accessibility();
    let node = find(&update, widget_id_to_node_id(minimap));

    // The useful half survives: it still says where the viewport is.
    assert_eq!(node.label(), Some("Scene minimap"));
    assert!(node.value().is_some());
    // The half with nothing behind it does not.
    assert!(
        tree.tab_stops_within(root).is_empty(),
        "a minimap with no navigation callback has nothing to activate; a Tab \
         stop with nothing behind it is worse than none"
    );
    for action in ROUTED_ACTIONS {
        assert!(
            !node.supports_action(action),
            "{action:?} must not be advertised by a minimap that cannot act on it"
        );
    }
}

#[test]
fn every_advertised_action_is_one_the_widget_actually_performs() {
    for action in ROUTED_ACTIONS {
        let mut f = Fixture::navigating();
        assert!(
            f.node().supports_action(action),
            "{action:?} is routed but not advertised — an AT client has no way \
             to discover it"
        );
        assert!(
            f.invoke(action),
            "{action:?} is advertised but nothing handled it — an AT client is \
             told the control will move and it does not"
        );
        assert_eq!(
            f.got.borrow().len(),
            1,
            "{action:?} must reach the app's navigation callback"
        );
    }
}

// ---------------------------------------------------------------------------
// 2. The value tracks the viewport
// ---------------------------------------------------------------------------

/// Drop the `AccessibilityOnly` binding and this reddens: a pan marks the
/// widget for repaint only, `a11y_dirty` stays clear, `sync_accessibility`
/// serves its cache, and the node goes on announcing the viewport the
/// minimap was built with.
#[test]
fn the_announced_value_follows_a_pan_with_no_rebuild() {
    let mut f = Fixture::navigating();
    let before = f.value();

    f.viewport.set(Rect::new(800.0, 100.0, 200.0, 200.0));

    let after = f.value();
    assert_ne!(
        before, after,
        "panning the view must change what the minimap announces"
    );
    assert_eq!(
        after,
        "Viewport at 90% across, 20% down; showing 20% of the width and 20% of the height"
    );
}

/// Zooming out past the content expands the effective extent, and the
/// readout is computed from that same extent — so the words describe the
/// picture that is actually on screen, not a frame the widget stopped
/// drawing.
#[test]
fn the_value_is_read_off_the_extent_the_picture_is_drawn_from() {
    let mut f = Fixture::navigating();
    // A viewport twice the content, centred on it: the extent grows to the
    // viewport, which therefore covers all of it.
    f.viewport.set(Rect::new(-500.0, -500.0, 2000.0, 2000.0));
    assert_eq!(
        f.value(),
        "Viewport at 50% across, 50% down; showing 100% of the width and 100% of the height"
    );
}

#[test]
fn a_degenerate_scene_still_reads_as_something() {
    let viewport = Signal::new(Rect::ZERO);
    let mm = SceneMinimap::new(Rect::ZERO, viewport).size(200.0, 200.0);
    let mut tree = WidgetTree::new();
    let minimap = tree.add(mm);
    tree.layout(PROPOSAL);
    let _ = tree.render();
    let node = find(&tree.sync_accessibility(), widget_id_to_node_id(minimap));
    assert_eq!(
        node.value(),
        Some("Viewport at 50% across, 50% down; showing 100% of the width and 100% of the height"),
        "an empty scene must not divide by zero into a NaN announcement"
    );
}

// ---------------------------------------------------------------------------
// 3. Three routes, one callback
// ---------------------------------------------------------------------------

#[test]
fn arrows_move_the_view_by_a_tenth_of_the_viewport() {
    // The viewport is 200 across and centred at (500, 500), so a nudge is 20.
    for (key, want) in [
        (Key::ArrowRight, Point::new(520.0, 500.0)),
        (Key::ArrowLeft, Point::new(480.0, 500.0)),
        (Key::ArrowDown, Point::new(500.0, 520.0)),
        (Key::ArrowUp, Point::new(500.0, 480.0)),
    ] {
        let mut f = Fixture::navigating();
        f.press(key, Modifiers::NONE);
        close(f.only_point(), want, &format!("{key:?}"));
    }
}

#[test]
fn shift_arrow_moves_a_whole_viewport() {
    let mut f = Fixture::navigating();
    f.press(Key::ArrowRight, Modifiers::SHIFT);
    close(
        f.only_point(),
        Point::new(700.0, 500.0),
        "Shift+ArrowRight is the page step — a whole viewport, not a tenth",
    );
}

#[test]
fn home_enter_and_space_centre_on_the_content() {
    for key in [Key::Home, Key::Enter, Key::Space] {
        let mut f = Fixture::navigating();
        // Wander off first, so "centre on the content" is a real move.
        f.viewport.set(Rect::new(5000.0, 5000.0, 200.0, 200.0));
        f.press(key, Modifiers::NONE);
        close(
            f.only_point(),
            CONTENT.center(),
            &format!("{key:?} must go home to the content, not to the current view"),
        );
    }
}

/// `Action::Click` is the position-free half of the tap: a click needs a
/// point, an AT client has none, so the primary action is the one
/// destination the widget can name on its own. Keyboard and AT must agree
/// about what that is.
#[test]
fn assistive_technology_recentres_exactly_where_the_keyboard_does() {
    let mut by_action = Fixture::navigating();
    by_action
        .viewport
        .set(Rect::new(5000.0, 5000.0, 200.0, 200.0));
    assert!(by_action.invoke(Action::Click));

    let mut by_key = Fixture::navigating();
    by_key.viewport.set(Rect::new(5000.0, 5000.0, 200.0, 200.0));
    by_key.press(Key::Home, Modifiers::NONE);

    close(
        by_action.only_point(),
        by_key.only_point(),
        "Action::Click and Home are the same command by two routes",
    );
}

#[test]
fn assistive_technology_scrolls_the_view_the_way_the_arrows_do() {
    for (action, key) in [
        (Action::ScrollLeft, Key::ArrowLeft),
        (Action::ScrollRight, Key::ArrowRight),
        (Action::ScrollUp, Key::ArrowUp),
        (Action::ScrollDown, Key::ArrowDown),
    ] {
        let mut by_action = Fixture::navigating();
        assert!(by_action.invoke(action), "{action:?} must be handled");
        let mut by_key = Fixture::navigating();
        by_key.press(key, Modifiers::NONE);
        close(
            by_action.only_point(),
            by_key.only_point(),
            &format!("{action:?} must mean exactly what {key:?} means"),
        );
    }
}

/// Only `Shift` is the minimap's. A chord the app bound elsewhere has to
/// bubble instead of being eaten by a focused minimap.
#[test]
fn a_modifier_chord_passes_through() {
    for modifiers in [Modifiers::CTRL, Modifiers::ALT, Modifiers::SUPER] {
        let mut f = Fixture::navigating();
        f.press(Key::ArrowRight, modifiers);
        assert!(
            f.got.borrow().is_empty(),
            "{modifiers:?}+ArrowRight is not the minimap's to claim"
        );
    }
}

// ---------------------------------------------------------------------------
// 4. The keyboard and AT routes say what they did — and the pointer does not
// ---------------------------------------------------------------------------

#[test]
fn a_keyboard_step_announces_where_it_left_the_viewport() {
    let mut f = Fixture::navigating();
    let since = f.announce_seq();
    f.press(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(
        f.last_announcement(since),
        "Viewport at 52% across, 50% down; showing 20% of the width and 20% of the height",
        "the announcement describes the move that was asked for — reading the \
         viewport signal back here would report the position just left, since \
         the app applies the pan after the handler returns"
    );
}

#[test]
fn an_assistive_technology_action_announces_too() {
    let mut f = Fixture::navigating();
    let since = f.announce_seq();
    assert!(f.invoke(Action::ScrollDown));
    assert_eq!(
        f.last_announcement(since),
        "Viewport at 50% across, 52% down; showing 20% of the width and 20% of the height"
    );
}

/// The pointer route moves the view and says nothing, for **every** pointer
/// kind.
///
/// This is the workspace rule, reached from both directions: `HsvCanvas` — the
/// other 2-D manipulator — announces from its arrows and its custom actions and
/// not from its drag or its tap, and the five data views' row reorder announces
/// from `common::ordered_move`, which is the *non-drag* alternative, while the
/// drop itself is silent. The one pointer route in the workspace that does
/// speak is the charts' readout, and only for a coarse pointer that pressed and
/// released without travelling — because that tap is an *inspection* standing in
/// for a hover a finger cannot perform, so the utterance is its whole product.
/// A minimap tap is a command whose destination the user chose by aiming at the
/// picture, and one people repeat; an utterance per click is a metronome over
/// whatever the user was reading.
///
/// Touch is covered as well as the mouse precisely because of that charts
/// exception: it applies to an inspection, not to a command, so a contact does
/// not buy the minimap's tap an utterance either.
#[test]
fn a_tap_moves_the_view_and_says_nothing() {
    for kind in [
        teksilo_tokens::PointerKind::Mouse,
        teksilo_tokens::PointerKind::Touch,
    ] {
        let mut f = Fixture::navigating();
        let since = f.announce_seq();
        let bounds = f.tree.bounds(f.minimap);
        f.tree.tap_with(kind, bounds.center());
        assert_eq!(
            f.got.borrow().len(),
            1,
            "{kind:?}: a tap must still reach the callback"
        );
        let _ = f.at();
        let said = f.tree.announcements_since(since);
        assert!(
            said.is_empty(),
            "{kind:?}: the pointer route must not push a live-region utterance,              got {said:?}"
        );
    }
}

/// …and the silence costs no information. The value on the node is the same
/// sentence every other route speaks, so a client that re-reads the control
/// after a click gets the new position — it is simply not interrupted with it.
///
/// This is the half that makes the test above a policy rather than a hole: a
/// silent route that also left the node stale would have taken the information
/// away rather than stopped pushing it.
///
/// It does **not** pin the `AccessibilityOnly` binding, and deliberately says
/// so: a tap moves focus onto the minimap, which requests an accessibility walk
/// by itself, so this passes with that binding removed. The binding is pinned
/// by `the_announced_value_follows_a_pan_with_no_rebuild`, where nothing but
/// the signal changes.
#[test]
fn what_the_tap_did_is_still_readable_on_the_node() {
    let mut f = Fixture::navigating();
    let before = f.value();

    // Up and to the leading side of centre, so both axes move.
    let bounds = f.tree.bounds(f.minimap);
    f.tree.tap_with(
        teksilo_tokens::PointerKind::Mouse,
        Point::new(
            bounds.x + bounds.width * 0.25,
            bounds.y + bounds.height * 0.25,
        ),
    );

    let asked_for = f.only_point();
    close(
        f.viewport.get().center(),
        asked_for,
        "the fixture's consumer centres the viewport on the requested point",
    );
    let after = f.value();
    assert_ne!(
        before, after,
        "a tap that moved the view must leave the node announcing the new          position when it is asked — being quiet is not being stale"
    );
    assert_eq!(
        after,
        "Viewport at 25% across, 25% down; showing 20% of the width and 20% of the height"
    );
}

// ---------------------------------------------------------------------------
// 5. The phrasing is the app's if it wants it
// ---------------------------------------------------------------------------

#[test]
fn access_readout_replaces_both_the_value_and_the_announcement() {
    let seen: Rc<RefCell<Vec<MinimapReadout>>> = Rc::new(RefCell::new(Vec::new()));
    let recorded = Rc::clone(&seen);
    let mut f = Fixture::build(move |mm| {
        let recorded = Rc::clone(&recorded);
        mm.access_readout(move |r: MinimapReadout| {
            recorded.borrow_mut().push(r);
            LocalizedString::literal(format!(
                "page {} of 10",
                (r.position.x * 10.0).round() as i32
            ))
        })
    });

    assert_eq!(f.value(), "page 5 of 10", "the value is the app's phrasing");

    let since = f.announce_seq();
    f.press(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(
        f.last_announcement(since),
        "page 5 of 10",
        "one closure serves both, so the value and the announcement cannot \
         drift into two different sentences"
    );

    let readouts = seen.borrow();
    let r = readouts.last().expect("the closure ran");
    assert_eq!(
        r.extent, CONTENT,
        "the extent handed over is the painted one"
    );
    assert_eq!(r.coverage.width, 0.2);
    assert_eq!(r.coverage.height, 0.2);
}

/// Not being focusable by default is a default, not a wall. An app that wants
/// its read-out in the Tab order anyway says so through the framework's
/// ordinary builder chain — the same `.focusable(true)` every widget has — so
/// the gate costs nobody the choice, and the widget needs no knob of its own
/// for it.
#[test]
fn a_read_only_minimap_can_still_be_put_in_the_tab_order_by_the_app() {
    use teksilo_core::widget_builder::WidgetBuilder;

    let viewport = Signal::new(VIEWPORT);
    let mm = SceneMinimap::new(CONTENT, viewport)
        .size(200.0, 200.0)
        .focusable(true);
    let mut tree = WidgetTree::new();
    let minimap = tree.add(mm);
    let root = tree.add(Padding::uniform(20.0).child_id(minimap));
    tree.layout(PROPOSAL);
    let _ = tree.render();

    assert_eq!(
        tree.tab_stops_within(root),
        vec![minimap],
        "`.focusable(true)` from the WidgetBuilder chain must reach this widget          like any other — the default is the framework's opinion, not a refusal"
    );
}

/// The framework's own override chain renames the node — no second set of
/// builders on this widget, and `tr!` therefore works unchanged.
#[test]
fn the_name_is_replaceable_through_the_framework_override_chain() {
    use teksilo_core::widget_builder::WidgetBuilder;

    let viewport = Signal::new(VIEWPORT);
    let mm = SceneMinimap::new(CONTENT, viewport)
        .size(200.0, 200.0)
        .access_label(lit!("Chapter map"));
    let mut tree = WidgetTree::new();
    let minimap = tree.add(mm);
    tree.layout(PROPOSAL);
    let _ = tree.render();
    let node = find(&tree.sync_accessibility(), widget_id_to_node_id(minimap));
    assert_eq!(node.label(), Some("Chapter map"));
    assert!(
        node.value().is_some(),
        "renaming must not cost the position readout"
    );
}
