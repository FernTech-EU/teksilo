// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The behavioural half of the drift guard: one `teksu!` block per builder
//! method the DSL learned, each written in the shape the missing-entry bug
//! actually breaks.
//!
//! The bug is not "the method does not work". It is that a property the
//! lowering pass does not recognise stays where the user wrote it, so the
//! `.child(..)` emitted *after* it resolves against `WidgetWithHandlers<T>`
//! instead of the widget. A block that only calls the method therefore proves
//! nothing. Each block below carries four items in one order: the new
//! property first, then a property only the unwrapped widget has, then a
//! child, then a property the reorder already knew about. If the new name is
//! not reordered, the block does not compile.
//!
//! The two failures the pair catches are different. A name missing from the
//! predicate is a **compile** failure, which is why every block is written in
//! the breaking shape. A reorder that compiles but loses an item is a
//! **runtime** failure, which is why each test then builds the tree and counts
//! the child.

use teksilo_canvas::SizeProposal;
use teksilo_core::widget::LayoutResponse;
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::{
    BuildContext, CancelReason, HitSlop, LayoutContext, LongPressRole, MultiContact,
    OverscrollBehavior, PanAxes, PanClaim, PointerInfo, TouchAction, Widget, WidgetId, WidgetTree,
};
use teksilo_macros::teksu;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A leaf with nothing but a size — the child whose arrival is the assertion.
#[derive(Debug, Default)]
struct Leaf;

impl Leaf {
    fn new() -> Self {
        Self
    }
}

impl Widget for Leaf {
    fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        proposal.resolve(10.0, 10.0).into()
    }
}

/// A container carrying the two things `WidgetWithHandlers<T>` does not
/// forward: an inherent `child` and an inherent `spacing`.
///
/// A local container rather than `VStack` on purpose — the failure being
/// pinned is a method-resolution one on the wrapper, which any widget with an
/// inherent `child` reproduces, and this keeps the fixture's dependency on
/// `teksilo-widgets` (and its build) out of a guard crate.
#[derive(Debug, Default)]
struct Probe {
    pending: Vec<Box<dyn Widget>>,
    spacing: f32,
    mounted: Vec<WidgetId>,
}

impl Probe {
    fn new() -> Self {
        Self::default()
    }

    fn child(mut self, w: impl Widget + 'static) -> Self {
        self.pending.push(Box::new(w));
        self
    }

    fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }
}

impl Widget for Probe {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.mounted = self.pending.drain(..).map(|w| ctx.add_boxed(w)).collect();
        self.mounted.clone()
    }

    fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        proposal.resolve(10.0, 10.0).into()
    }

    fn children(&self) -> Vec<WidgetId> {
        self.mounted.clone()
    }
}

/// A root whose `build` runs one of the `teksu!` blocks below, so each block
/// is exercised through a real `BuildContext`.
#[derive(Debug)]
struct Root(fn(&mut BuildContext) -> WidgetId);

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        vec![(self.0)(ctx)]
    }

    fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        proposal.resolve(10.0, 10.0).into()
    }
}

/// Build `f`'s block into a real tree and assert its single child arrived.
///
/// Catches the failure the compiler cannot: a reorder that partitions the body
/// but drops an item still type-checks, and the child simply never reaches the
/// widget.
fn assert_lowers(f: fn(&mut BuildContext) -> WidgetId) {
    let mut tree = WidgetTree::new();
    let root = tree.add(Root(f));
    tree.layout(SizeProposal::exact(200.0, 200.0));

    let probe = *tree
        .children(root)
        .first()
        .expect("the teksu block must have produced a widget");
    assert_eq!(
        tree.children(probe).len(),
        1,
        "the block's child did not reach the widget — the property was attached \
         to the `WidgetWithHandlers` wrapper instead"
    );
}

// ---------------------------------------------------------------------------
// One block per method the guard added, each in the breaking shape
// ---------------------------------------------------------------------------

fn touch_action_block(ctx: &mut BuildContext) -> WidgetId {
    teksu!(ctx => Probe {
        touch_action: TouchAction::NONE
        spacing: 4.0
        Leaf
        on_tap: |_, _ctx| {}
    })
}

fn scroll_container_block(ctx: &mut BuildContext) -> WidgetId {
    teksu!(ctx => Probe {
        scroll_container: PanAxes::Y
        spacing: 4.0
        Leaf
        on_tap: |_, _ctx| {}
    })
}

fn pan_claim_block(ctx: &mut BuildContext) -> WidgetId {
    teksu!(ctx => Probe {
        pan_claim: PanClaim::vertical()
        spacing: 4.0
        Leaf
        on_tap: |_, _ctx| {}
    })
}

fn overscroll_behavior_block(ctx: &mut BuildContext) -> WidgetId {
    teksu!(ctx => Probe {
        overscroll_behavior: OverscrollBehavior::Contain
        spacing: 4.0
        Leaf
        on_tap: |_, _ctx| {}
    })
}

fn multi_contact_block(ctx: &mut BuildContext) -> WidgetId {
    teksu!(ctx => Probe {
        multi_contact: MultiContact::All
        spacing: 4.0
        Leaf
        on_tap: |_, _ctx| {}
    })
}

fn long_press_role_block(ctx: &mut BuildContext) -> WidgetId {
    teksu!(ctx => Probe {
        long_press_role: LongPressRole::ContextMenu
        spacing: 4.0
        Leaf
        on_tap: |_, _ctx| {}
    })
}

/// The struct literal is parenthesised because an unwrapped `HitSlop { .. }`
/// at argument position is a teksu *element* — an UpperCamel head with a body —
/// and would lower to `HitSlop::new(..)`. Parens are the documented escape.
fn hit_slop_block(ctx: &mut BuildContext) -> WidgetId {
    teksu!(ctx => Probe {
        hit_slop: (HitSlop { radius: 8.0, up_to: 24.0 })
        spacing: 4.0
        Leaf
        on_tap: |_, _ctx| {}
    })
}

/// `no_hit_slop` takes no argument, so it reaches the DSL as a bare lowercase
/// ident at body position — the one surface form whose reorder is otherwise
/// untested for a wrapping method.
fn no_hit_slop_block(ctx: &mut BuildContext) -> WidgetId {
    teksu!(ctx => Probe {
        no_hit_slop
        spacing: 4.0
        Leaf
        on_tap: |_, _ctx| {}
    })
}

fn drag_activation_block(ctx: &mut BuildContext) -> WidgetId {
    teksu!(ctx => Probe {
        drag_activation: teksilo_tokens::DragActivation::AfterLongPress
        spacing: 4.0
        Leaf
        on_tap: |_, _ctx| {}
    })
}

fn gesture_dead_zone_block(ctx: &mut BuildContext) -> WidgetId {
    teksu!(ctx => Probe {
        gesture_dead_zone: true
        spacing: 4.0
        Leaf
        on_tap: |_, _ctx| {}
    })
}

fn keyboard_capture_block(ctx: &mut BuildContext) -> WidgetId {
    teksu!(ctx => Probe {
        keyboard_capture: true
        spacing: 4.0
        Leaf
        on_tap: |_, _ctx| {}
    })
}

fn on_pointer_cancel_block(ctx: &mut BuildContext) -> WidgetId {
    teksu!(ctx => Probe {
        on_pointer_cancel: |_: &PointerInfo, _: CancelReason, _ctx: &mut _| {}
        spacing: 4.0
        Leaf
        on_tap: |_, _ctx| {}
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn touch_action_lowers_before_a_child_and_a_handler() {
    assert_lowers(touch_action_block);
}

#[test]
fn scroll_container_lowers_before_a_child_and_a_handler() {
    assert_lowers(scroll_container_block);
}

#[test]
fn pan_claim_lowers_before_a_child_and_a_handler() {
    assert_lowers(pan_claim_block);
}

#[test]
fn overscroll_behavior_lowers_before_a_child_and_a_handler() {
    assert_lowers(overscroll_behavior_block);
}

#[test]
fn multi_contact_lowers_before_a_child_and_a_handler() {
    assert_lowers(multi_contact_block);
}

#[test]
fn long_press_role_lowers_before_a_child_and_a_handler() {
    assert_lowers(long_press_role_block);
}

#[test]
fn hit_slop_lowers_before_a_child_and_a_handler() {
    assert_lowers(hit_slop_block);
}

#[test]
fn no_hit_slop_lowers_before_a_child_and_a_handler() {
    assert_lowers(no_hit_slop_block);
}

#[test]
fn drag_activation_lowers_before_a_child_and_a_handler() {
    assert_lowers(drag_activation_block);
}

#[test]
fn gesture_dead_zone_lowers_before_a_child_and_a_handler() {
    assert_lowers(gesture_dead_zone_block);
}

#[test]
fn keyboard_capture_lowers_before_a_child_and_a_handler() {
    assert_lowers(keyboard_capture_block);
}

#[test]
fn on_pointer_cancel_lowers_before_a_child_and_a_handler() {
    assert_lowers(on_pointer_cancel_block);
}
