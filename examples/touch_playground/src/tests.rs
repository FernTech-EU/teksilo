// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The playground's scenarios, driven headlessly.
//!
//! Input comes in as real `AutomationOp`s through `teksilo_automation::execute`
//! — the same door an agent drives the running window through — rather than
//! through the tree's own touch helpers. That is deliberate: it means a change
//! that breaks an agent's route into the playground breaks this file too, and a
//! hardware session's first step (`teksilo-automation-mcp` against the running
//! app) is already covered before anyone plugs a tablet in.
//!
//! What each test asserts is the **outcome string the scenario published** —
//! exactly what the on-screen readout shows and what
//! `docs/touch-verification.md` tells a tester to look at. So a scenario that
//! stops reporting fails here, in a second, instead of in a hardware session.
//!
//! # What these tests are not
//!
//! They are not the arbitration matrix. `crates/teksilo-core/tests/
//! arbitration_matrix.rs` reads the *tree's* own answer — who won, who was
//! cancelled and with what reason — against core-only fixtures. These read what
//! a real widget did about it. Both are worth having and neither replaces the
//! other: a fixture can be right while the widget built on that shape is wrong.

use std::time::Duration;

use teksilo::canvas::SizeProposal;
use teksilo::core::WidgetTree;
use teksilo::core::widget_id::WidgetId;
use teksilo::tokens::TargetDensity;
use teksilo_automation::dto::{
    AutomationOp, AutomationReply, LayoutNode, SettleSpec, TouchPhaseDto, TouchStep,
};
use teksilo_automation::executor::execute;
use teksilo_automation::recording_ops::RecordingWindowOps;

use crate::Root;
use crate::state::{
    PlaygroundState, SCENARIO_LIST_ROW, SCENARIO_NESTED, SCENARIO_SLIDER, ScenarioId,
};

/// The window the tests lay the playground out in.
///
/// Deliberately far taller than any real one. The scenario column is a
/// `ScrollArea`, so in a realistic window most subjects are below the viewport
/// and a point at their centre is outside the tree's bounds — the injected
/// contact would land on nothing and every assertion would fail for a reason
/// that has nothing to do with arbitration. A headless layout pays nothing for
/// height, so the whole column is on screen and each test drives the widget it
/// names.
const WINDOW: (f32, f32) = (1180.0, 2800.0);

/// A laid-out playground, plus a handle on the state its scenarios report into.
struct Harness {
    tree: WidgetTree,
    ops: RecordingWindowOps,
    state: PlaygroundState,
}

impl Harness {
    fn new() -> Self {
        Self::at_density(TargetDensity::Compact)
    }

    fn at_density(density: TargetDensity) -> Self {
        // A real text backend is not available headlessly; the mock gives every
        // glyph a fixed advance, which is all any of these tests needs — none
        // asserts a text position.
        let mut tree = WidgetTree::new().with_text_backend(std::rc::Rc::new(
            std::cell::RefCell::new(teksilo::canvas::MockTextBackend::new()),
        ));
        tree.set_theme(teksilo::presets::intui::light().with_density(density));
        let root = Root::new(density);
        let state = root.state.clone();
        tree.add(root);
        let mut harness = Harness {
            tree,
            ops: RecordingWindowOps::new(),
            state,
        };
        harness.lay_out();
        harness
    }

    fn lay_out(&mut self) {
        self.tree.layout(SizeProposal::exact(WINDOW.0, WINDOW.1));
    }

    fn run(&mut self, op: AutomationOp) -> serde_json::Value {
        let reply = execute(&mut self.tree, &mut self.ops, &op, &SettleSpec::default());
        match reply {
            AutomationReply::Ok { data } => data,
            other => panic!("{op:?} failed: {other:?}"),
        }
    }

    /// Every widget in the layout tree whose Rust type path contains `needle`.
    ///
    /// The layout tree rather than the accessibility tree, because half the
    /// subjects here are layout primitives or panes the AT walker prunes.
    fn layout_nodes(&mut self, needle: &str) -> Vec<LayoutNode> {
        let data = self.run(AutomationOp::LayoutTree {
            max_depth: None,
            include_debug: false,
        });
        let nodes: Vec<LayoutNode> =
            serde_json::from_value(data["nodes"].clone()).expect("a layout-node list");
        nodes
            .into_iter()
            .filter(|n| n.type_name.contains(needle))
            .collect()
    }

    /// The centre of the first laid-out, active widget whose type path contains
    /// `needle`, in window-logical coordinates.
    fn centre_of(&mut self, needle: &str) -> (f32, f32) {
        let node = self
            .layout_nodes(needle)
            .into_iter()
            .find(|n| n.active && n.bounds.width > 1.0 && n.bounds.height > 1.0)
            .unwrap_or_else(|| panic!("no laid-out widget matching {needle:?}"));
        (
            (node.bounds.x + node.bounds.width / 2.0) as f32,
            (node.bounds.y + node.bounds.height / 2.0) as f32,
        )
    }

    /// The centre of the first row of the list-row scenario's `ListView`.
    ///
    /// Not the view's centre: see [`touch_hold_drag`] for why where inside a row
    /// a press lands decides whether a deferred drag can latch.
    fn first_row_centre(&mut self) -> (f32, f32) {
        let node = self
            .layout_nodes("list_view::ListView")
            .into_iter()
            .find(|n| n.active && n.bounds.height > 1.0)
            .expect("the list-row scenario is laid out");
        (
            (node.bounds.x + node.bounds.width / 2.0) as f32,
            node.bounds.y as f32 + crate::scenarios::LIST_ITEM_HEIGHT / 2.0,
        )
    }

    fn outcome(&self, scenario: ScenarioId) -> String {
        self.state.outcome(scenario)
    }

    /// Whether `scenario` reported an outcome starting with `prefix` at any
    /// point in this session.
    ///
    /// The final verdict is not always the right assertion: a mouse drag that
    /// reorders a row also moves the selection to it, so the *last* report is
    /// the selection and the reorder is one line above it. The log carries every
    /// report in order, which is what "who won this gesture" actually needs.
    fn reported(&self, scenario: ScenarioId, prefix: &str) -> bool {
        let wanted = format!("{scenario}: {prefix}");
        self.state
            .events
            .get()
            .iter()
            .any(|l| l.starts_with(&wanted))
    }

    fn log(&self) -> Vec<String> {
        self.state.events.get()
    }
}

/// A touch drag: down, `steps` moves toward the offset, up. `hold_ms` of
/// simulated time is spent still on the very first sample, which is what arms a
/// deferred drag.
fn touch_drag(
    from: (f32, f32),
    by: (f32, f32),
    steps: usize,
    hold_ms: u64,
    release: bool,
) -> Vec<TouchStep> {
    let mut out = vec![TouchStep {
        contact: 0,
        phase: TouchPhaseDto::Down,
        x: from.0,
        y: from.1,
        advance_ms: 0,
    }];
    if hold_ms > 0 {
        // A still sample at the press point after the hold: the deadline is
        // reached by the clock, and this is the sample that observes it.
        out.push(TouchStep {
            contact: 0,
            phase: TouchPhaseDto::Move,
            x: from.0,
            y: from.1,
            advance_ms: hold_ms,
        });
    }
    for step in 1..=steps {
        let t = step as f32 / steps as f32;
        out.push(TouchStep {
            contact: 0,
            phase: TouchPhaseDto::Move,
            x: from.0 + by.0 * t,
            y: from.1 + by.1 * t,
            // One 60 Hz frame per sample, so the velocity tracker sees gaps
            // under its stop threshold.
            advance_ms: 16,
        });
    }
    if release {
        let last = out.last().expect("the sequence has a down");
        out.push(TouchStep {
            contact: 0,
            phase: TouchPhaseDto::Up,
            x: last.x,
            y: last.y,
            advance_ms: 16,
        });
    }
    out
}

/// The hold a deferred drag waits out, plus a margin, in simulated ms.
fn hold_ms() -> u64 {
    crate::scenarios::long_press_millis() + 40
}

/// Hold, then drag: down, a still sample past the long-press deadline, one
/// sample that clears `drag_slop` **without leaving the row**, then the rest of
/// the travel, then up.
///
/// The middle step is the load-bearing one and it is not an implementation
/// detail of this helper. A deferred drag is revoked if the first sample after
/// the hold lands outside the pressed row (`docs/data-view-touch.md` §2), so a
/// gesture that reaches the destination in even steps of less than `drag_slop`
/// leaves the row before it ever latches — which is what the scroller then wins.
/// Aiming the press at a row centre and crossing the slop in one jump is what a
/// real finger does anyway.
fn touch_hold_drag(
    from: (f32, f32),
    latch_by: f32,
    remaining: f32,
    steps: usize,
) -> Vec<TouchStep> {
    let mut out = vec![
        TouchStep {
            contact: 0,
            phase: TouchPhaseDto::Down,
            x: from.0,
            y: from.1,
            advance_ms: 0,
        },
        TouchStep {
            contact: 0,
            phase: TouchPhaseDto::Move,
            x: from.0,
            y: from.1,
            advance_ms: hold_ms(),
        },
        TouchStep {
            contact: 0,
            phase: TouchPhaseDto::Move,
            x: from.0,
            y: from.1 + latch_by,
            advance_ms: 16,
        },
    ];
    for step in 1..=steps {
        let t = step as f32 / steps as f32;
        out.push(TouchStep {
            contact: 0,
            phase: TouchPhaseDto::Move,
            x: from.0,
            y: from.1 + latch_by + remaining * t,
            advance_ms: 16,
        });
    }
    let last = out.last().expect("the sequence has a down");
    out.push(TouchStep {
        contact: 0,
        phase: TouchPhaseDto::Up,
        x: last.x,
        y: last.y,
        advance_ms: 16,
    });
    out
}

#[test]
fn the_pad_reports_a_contact_with_its_frozen_action_and_no_pan() {
    let mut h = Harness::new();
    let pad = h.centre_of("PointerPad");
    // Left down, not released: a sequence is only observable while it is live.
    h.run(AutomationOp::InjectTouchSequence {
        steps: touch_drag(pad, (0.0, 30.0), 3, 0, false),
    });

    let rows = h.state.pointers.get();
    assert_eq!(
        rows.len(),
        1,
        "one finger is down, so the pad reports exactly one pointer: {rows:?}"
    );
    let row = &rows[0];
    assert_eq!(
        row.kind,
        teksilo::tokens::PointerKind::Touch,
        "the injected contact must arrive as a touch, not as a mouse: {row:?}"
    );
    assert!(row.down, "the contact was never lifted: {row:?}");
    assert_eq!(
        row.touch_action, "NONE",
        "the pad declares TouchAction::NONE, so that is what its press freezes"
    );
    assert!(
        row.speed > 0.0,
        "three moves over two frames give the pad's own tracker a velocity: {row:?}"
    );
}

#[test]
fn a_lifted_contact_leaves_the_pads_list() {
    let mut h = Harness::new();
    let pad = h.centre_of("PointerPad");
    h.run(AutomationOp::InjectTouchSequence {
        steps: touch_drag(pad, (0.0, 20.0), 2, 0, true),
    });
    assert!(
        h.state.pointers.get().is_empty(),
        "a contact ceases to exist when it lifts: {:?}",
        h.state.pointers.get()
    );
}

#[test]
fn two_fingers_on_the_pad_are_both_reported() {
    let mut h = Harness::new();
    let (x, y) = h.centre_of("PointerPad");
    let steps = vec![
        TouchStep {
            contact: 0,
            phase: TouchPhaseDto::Down,
            x: x - 30.0,
            y,
            advance_ms: 0,
        },
        TouchStep {
            contact: 1,
            phase: TouchPhaseDto::Down,
            x: x + 30.0,
            y,
            advance_ms: 16,
        },
    ];
    h.run(AutomationOp::InjectTouchSequence { steps });
    let rows = h.state.pointers.get();
    assert_eq!(
        rows.len(),
        2,
        "the pad declares MultiContact::All, so the second finger is delivered \
         rather than terminated at the node: {rows:?}"
    );
    assert_ne!(
        rows[0].id, rows[1].id,
        "each contact is minted its own identity: {rows:?}"
    );
}

#[test]
fn a_short_finger_drag_on_a_list_pans_it_rather_than_reordering() {
    let mut h = Harness::new();
    let list = h.centre_of("list_view::ListView");
    h.run(AutomationOp::InjectTouchSequence {
        steps: touch_drag(list, (0.0, -80.0), 6, 0, true),
    });
    let outcome = h.outcome(SCENARIO_LIST_ROW);
    assert!(
        outcome.starts_with("pan"),
        "a finger's drag with no hold belongs to the scroller, not to the row's \
         reorder: got {outcome:?}"
    );
}

#[test]
fn a_finger_that_holds_first_reorders_the_row_instead() {
    let mut h = Harness::new();
    let row = h.first_row_centre();
    // 19 dp clears the touch profile's 18 dp `drag_slop` in one sample and stays
    // inside a 40 dp row pressed at its centre.
    h.run(AutomationOp::InjectTouchSequence {
        steps: touch_hold_drag(row, 19.0, 81.0, 6),
    });
    assert!(
        h.reported(SCENARIO_LIST_ROW, "reorder"),
        "the hold is what arms a direct pointer's item drag: the list reported \
         {:?}",
        h.log()
    );
}

#[test]
fn a_mouse_drag_on_the_same_list_reorders_with_no_hold() {
    let mut h = Harness::new();
    let list = h.centre_of("list_view::ListView");
    // `DragNode` is the mouse route by construction: press, travel, release, all
    // on the singular mouse pointer, with no hold anywhere in it.
    let node = h
        .layout_nodes("list_view::ListView")
        .into_iter()
        .find(|n| n.active)
        .expect("the list is laid out");
    h.run(AutomationOp::DragNode {
        node: node.id,
        to_node: None,
        to_x: Some(list.0),
        to_y: Some(list.1 + 100.0),
    });
    assert!(
        h.reported(SCENARIO_LIST_ROW, "reorder"),
        "a mouse never enrols the pan claimant, so its drag is the row's from \
         the first 5 dp: the list reported {:?}",
        h.log()
    );
}

#[test]
fn a_finger_on_the_slider_moves_the_slider_and_never_pans() {
    let mut h = Harness::new();
    let slider = h.centre_of("slider::Slider");
    h.run(AutomationOp::InjectTouchSequence {
        steps: touch_drag(slider, (60.0, 0.0), 6, 0, true),
    });
    let outcome = h.outcome(SCENARIO_SLIDER);
    assert!(
        outcome.starts_with("the slider"),
        "the slider declares TouchAction::NONE, which keeps the enclosing \
         claimant out of the member list entirely: got {outcome:?}"
    );
}

#[test]
fn a_finger_pan_reaches_the_inner_list_of_the_nested_pair() {
    let mut h = Harness::new();
    // The nested scenario's inner list is the second `ListView` in the column.
    let lists = h.layout_nodes("list_view::ListView");
    let inner = lists
        .iter()
        .filter(|n| n.active && n.bounds.height > 1.0)
        .nth(1)
        .expect("the nested scenario contributes a second list");
    let from = (
        (inner.bounds.x + inner.bounds.width / 2.0) as f32,
        (inner.bounds.y + inner.bounds.height / 2.0) as f32,
    );
    h.run(AutomationOp::InjectTouchSequence {
        steps: touch_drag(from, (0.0, -60.0), 6, 0, true),
    });
    let outcome = h.outcome(SCENARIO_NESTED);
    assert!(
        outcome.contains("inner"),
        "a pan that starts inside the inner list is the inner list's until it \
         runs out: got {outcome:?}"
    );
}

#[test]
fn the_readout_names_the_three_gesture_profiles_at_the_active_density() {
    // The readout is the pad's other half, and it is what a tester reads the
    // ladder off. Its lines are produced in `layout_response`, so a laid-out
    // tree already has them.
    for density in [
        TargetDensity::Compact,
        TargetDensity::Comfortable,
        TargetDensity::Touch,
    ] {
        let h = Harness::at_density(density);
        let tokens = teksilo::tokens::InputTokens::for_density(density);
        // The readout paints its own text, so what it says is read back through
        // the accessibility name it publishes — the same string.
        let names = collect_labels(&h.tree);
        let readout = names
            .iter()
            .find(|n| n.contains("profile        tap"))
            .unwrap_or_else(|| panic!("the readout publishes its lines as its AT name: {names:?}"));
        assert!(
            readout.contains(&format!("target {} dp", tokens.target_size)),
            "the readout must name the density's own target size ({} dp) at {density:?}: {readout}",
            tokens.target_size
        );
        for profile in ["mouse", "touch", "pen"] {
            assert!(
                readout.contains(profile),
                "all three profiles are shown side by side; {profile} is missing: {readout}"
            );
        }
    }
}

#[test]
fn the_density_toggle_is_reachable_and_starts_where_it_was_asked_to() {
    let h = Harness::at_density(TargetDensity::Touch);
    assert_eq!(
        h.state.density.get(),
        TargetDensity::Touch,
        "`--density touch` seeds the toggle, so a tablet session does not begin \
         by rebuilding the tree"
    );
}

/// Every accessibility name in the tree, for the painted-text panels whose
/// content is only reachable that way.
fn collect_labels(tree: &WidgetTree) -> Vec<String> {
    let mut out = Vec::new();
    for root in tree.roots() {
        walk_labels(tree, root, &mut out);
    }
    out
}

fn walk_labels(tree: &WidgetTree, id: WidgetId, out: &mut Vec<String>) {
    if let Some(name) = tree.accessibility_node(id).name() {
        out.push(name.to_string());
    }
    for child in tree.children(id) {
        walk_labels(tree, child, out);
    }
}

/// The hold threshold the playground shows is the one the tokens carry, not a
/// number typed into a label.
#[test]
fn the_advertised_hold_is_the_touch_profiles_own() {
    assert_eq!(
        Duration::from_millis(crate::scenarios::long_press_millis()),
        teksilo::tokens::GestureProfile::TOUCH.long_press,
        "the instruction text reads the threshold off the profile"
    );
}
