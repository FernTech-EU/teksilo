//! Branching widget types and child-insertion trait for the `fern!` DSL.
//!
//! `FernBranch{,3,4}` are two-, three-, and four-way sum types over Widget
//! implementations. They exist so `if`/`else` and small `match` arms in
//! `fern!` can yield heterogeneous widget types from the same position
//! without boxing. Each variant implements Widget by delegating every
//! method to the active arm.
//!
//! `IntoFernChild` is the dispatch trait the macro uses when it cannot
//! decide at expansion time whether a child expression is a widget value
//! or a pre-registered `WidgetId` (the `#{ expr }` escape case). It
//! produces a `PendingChild`, which Category A containers already know
//! how to route through their `child()` / `add_child()` path.

use fern_canvas::{Canvas, Rect, Size, SizeProposal};

use crate::accessibility::AccessNodeBuilder;
use crate::build_context::BuildContext;
use crate::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use crate::widget_builder::HandlerSet;
use crate::widget_id::WidgetId;

// ---------------------------------------------------------------------------
// FernBranch — two-way sum type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum FernBranch<L: Widget, R: Widget> {
    L(L),
    R(R),
}

impl<L: Widget, R: Widget> Widget for FernBranch<L, R> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        match self {
            FernBranch::L(w) => w.build(ctx),
            FernBranch::R(w) => w.build(ctx),
        }
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> crate::widget::LayoutResponse {
        match self {
            FernBranch::L(w) => w.layout_response(proposal, ctx),
            FernBranch::R(w) => w.layout_response(proposal, ctx),
        }.into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        match self {
            FernBranch::L(w) => w.place_children(bounds, proposal, children, ctx),
            FernBranch::R(w) => w.place_children(bounds, proposal, children, ctx),
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        match self {
            FernBranch::L(w) => w.paint(bounds, canvas, ctx),
            FernBranch::R(w) => w.paint(bounds, canvas, ctx),
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        match self {
            FernBranch::L(w) => w.accessibility(builder),
            FernBranch::R(w) => w.accessibility(builder),
        }
    }

    fn accessible_title_hint(&self) -> Option<String> {
        match self {
            FernBranch::L(w) => w.accessible_title_hint(),
            FernBranch::R(w) => w.accessible_title_hint(),
        }
    }

    fn initial_focus_hint(&self) -> Option<WidgetId> {
        match self {
            FernBranch::L(w) => w.initial_focus_hint(),
            FernBranch::R(w) => w.initial_focus_hint(),
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        match self {
            FernBranch::L(w) => w.children(),
            FernBranch::R(w) => w.children(),
        }
    }

    fn clips_children(&self) -> bool {
        match self {
            FernBranch::L(w) => w.clips_children(),
            FernBranch::R(w) => w.clips_children(),
        }
    }

    fn take_handler_set(&mut self) -> Option<HandlerSet> {
        match self {
            FernBranch::L(w) => w.take_handler_set(),
            FernBranch::R(w) => w.take_handler_set(),
        }
    }
}

// ---------------------------------------------------------------------------
// FernBranch3 — three-way sum type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum FernBranch3<A: Widget, B: Widget, C: Widget> {
    A(A),
    B(B),
    C(C),
}

impl<A: Widget, B: Widget, C: Widget> Widget for FernBranch3<A, B, C> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        match self {
            FernBranch3::A(w) => w.build(ctx),
            FernBranch3::B(w) => w.build(ctx),
            FernBranch3::C(w) => w.build(ctx),
        }
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> crate::widget::LayoutResponse {
        match self {
            FernBranch3::A(w) => w.layout_response(proposal, ctx),
            FernBranch3::B(w) => w.layout_response(proposal, ctx),
            FernBranch3::C(w) => w.layout_response(proposal, ctx),
        }.into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        match self {
            FernBranch3::A(w) => w.place_children(bounds, proposal, children, ctx),
            FernBranch3::B(w) => w.place_children(bounds, proposal, children, ctx),
            FernBranch3::C(w) => w.place_children(bounds, proposal, children, ctx),
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        match self {
            FernBranch3::A(w) => w.paint(bounds, canvas, ctx),
            FernBranch3::B(w) => w.paint(bounds, canvas, ctx),
            FernBranch3::C(w) => w.paint(bounds, canvas, ctx),
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        match self {
            FernBranch3::A(w) => w.accessibility(builder),
            FernBranch3::B(w) => w.accessibility(builder),
            FernBranch3::C(w) => w.accessibility(builder),
        }
    }

    fn accessible_title_hint(&self) -> Option<String> {
        match self {
            FernBranch3::A(w) => w.accessible_title_hint(),
            FernBranch3::B(w) => w.accessible_title_hint(),
            FernBranch3::C(w) => w.accessible_title_hint(),
        }
    }

    fn initial_focus_hint(&self) -> Option<WidgetId> {
        match self {
            FernBranch3::A(w) => w.initial_focus_hint(),
            FernBranch3::B(w) => w.initial_focus_hint(),
            FernBranch3::C(w) => w.initial_focus_hint(),
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        match self {
            FernBranch3::A(w) => w.children(),
            FernBranch3::B(w) => w.children(),
            FernBranch3::C(w) => w.children(),
        }
    }

    fn clips_children(&self) -> bool {
        match self {
            FernBranch3::A(w) => w.clips_children(),
            FernBranch3::B(w) => w.clips_children(),
            FernBranch3::C(w) => w.clips_children(),
        }
    }

    fn take_handler_set(&mut self) -> Option<HandlerSet> {
        match self {
            FernBranch3::A(w) => w.take_handler_set(),
            FernBranch3::B(w) => w.take_handler_set(),
            FernBranch3::C(w) => w.take_handler_set(),
        }
    }
}

// ---------------------------------------------------------------------------
// FernBranch4 — four-way sum type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum FernBranch4<A: Widget, B: Widget, C: Widget, D: Widget> {
    A(A),
    B(B),
    C(C),
    D(D),
}

impl<A: Widget, B: Widget, C: Widget, D: Widget> Widget for FernBranch4<A, B, C, D> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        match self {
            FernBranch4::A(w) => w.build(ctx),
            FernBranch4::B(w) => w.build(ctx),
            FernBranch4::C(w) => w.build(ctx),
            FernBranch4::D(w) => w.build(ctx),
        }
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> crate::widget::LayoutResponse {
        match self {
            FernBranch4::A(w) => w.layout_response(proposal, ctx),
            FernBranch4::B(w) => w.layout_response(proposal, ctx),
            FernBranch4::C(w) => w.layout_response(proposal, ctx),
            FernBranch4::D(w) => w.layout_response(proposal, ctx),
        }.into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        match self {
            FernBranch4::A(w) => w.place_children(bounds, proposal, children, ctx),
            FernBranch4::B(w) => w.place_children(bounds, proposal, children, ctx),
            FernBranch4::C(w) => w.place_children(bounds, proposal, children, ctx),
            FernBranch4::D(w) => w.place_children(bounds, proposal, children, ctx),
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        match self {
            FernBranch4::A(w) => w.paint(bounds, canvas, ctx),
            FernBranch4::B(w) => w.paint(bounds, canvas, ctx),
            FernBranch4::C(w) => w.paint(bounds, canvas, ctx),
            FernBranch4::D(w) => w.paint(bounds, canvas, ctx),
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        match self {
            FernBranch4::A(w) => w.accessibility(builder),
            FernBranch4::B(w) => w.accessibility(builder),
            FernBranch4::C(w) => w.accessibility(builder),
            FernBranch4::D(w) => w.accessibility(builder),
        }
    }

    fn accessible_title_hint(&self) -> Option<String> {
        match self {
            FernBranch4::A(w) => w.accessible_title_hint(),
            FernBranch4::B(w) => w.accessible_title_hint(),
            FernBranch4::C(w) => w.accessible_title_hint(),
            FernBranch4::D(w) => w.accessible_title_hint(),
        }
    }

    fn initial_focus_hint(&self) -> Option<WidgetId> {
        match self {
            FernBranch4::A(w) => w.initial_focus_hint(),
            FernBranch4::B(w) => w.initial_focus_hint(),
            FernBranch4::C(w) => w.initial_focus_hint(),
            FernBranch4::D(w) => w.initial_focus_hint(),
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        match self {
            FernBranch4::A(w) => w.children(),
            FernBranch4::B(w) => w.children(),
            FernBranch4::C(w) => w.children(),
            FernBranch4::D(w) => w.children(),
        }
    }

    fn clips_children(&self) -> bool {
        match self {
            FernBranch4::A(w) => w.clips_children(),
            FernBranch4::B(w) => w.clips_children(),
            FernBranch4::C(w) => w.clips_children(),
            FernBranch4::D(w) => w.clips_children(),
        }
    }

    fn take_handler_set(&mut self) -> Option<HandlerSet> {
        match self {
            FernBranch4::A(w) => w.take_handler_set(),
            FernBranch4::B(w) => w.take_handler_set(),
            FernBranch4::C(w) => w.take_handler_set(),
            FernBranch4::D(w) => w.take_handler_set(),
        }
    }
}

// ---------------------------------------------------------------------------
// IntoFernChild — widget-or-id dispatch for #{ expr } child positions
// ---------------------------------------------------------------------------

/// Dispatch trait the `fern!` macro uses to route child expressions whose
/// static type isn't known at expansion time (the `#{ expr }` escape).
/// `impl Widget + 'static` values lower to `PendingChild::Deferred`;
/// pre-registered `WidgetId` values lower to `PendingChild::Id`.
pub trait IntoFernChild {
    fn into_pending(self) -> PendingChild;
}

impl<W: Widget + 'static> IntoFernChild for W {
    fn into_pending(self) -> PendingChild {
        PendingChild::Deferred(Box::new(self))
    }
}

impl IntoFernChild for WidgetId {
    fn into_pending(self) -> PendingChild {
        PendingChild::Id(self)
    }
}

// ---------------------------------------------------------------------------
// IntoFernCondition — reactive/static dispatch for `if bare_ident { ... }`
// ---------------------------------------------------------------------------

/// Dispatch trait the `fern!` macro uses for `if bare_ident { Element }`
/// — spec §5.1 "reactive conditionals". The bare-identifier form
/// lowers to a call on this trait; which impl fires (and thus whether
/// the element is conditionally built or always built with bound
/// visibility) is decided at monomorphization.
///
/// - `bool`: static — the element is built only when the flag is true.
///   Returns `Some(id)` if built, `None` if skipped.
/// - `Signal<bool>` / `Prop<bool>`: reactive — the element is always
///   built, and its visibility is bound to the signal via
///   `BuildContext::visible_when`. Returns `Some(id)` unconditionally.
///
/// The return type is `Option<WidgetId>` so the macro can use a single
/// lowering shape (`if let Some(id) = ... { parent.add_child(id) }`)
/// that works for both cases.
pub trait IntoFernCondition {
    fn fern_into_conditional_child<W: crate::widget::Widget + 'static>(
        self,
        child: W,
        ctx: &mut crate::build_context::BuildContext,
    ) -> Option<WidgetId>;
}

impl IntoFernCondition for bool {
    fn fern_into_conditional_child<W: crate::widget::Widget + 'static>(
        self,
        child: W,
        ctx: &mut crate::build_context::BuildContext,
    ) -> Option<WidgetId> {
        if self { Some(ctx.add(child)) } else { None }
    }
}

impl IntoFernCondition for crate::signal::Signal<bool> {
    fn fern_into_conditional_child<W: crate::widget::Widget + 'static>(
        self,
        child: W,
        ctx: &mut crate::build_context::BuildContext,
    ) -> Option<WidgetId> {
        let id = ctx.add(child);
        ctx.visible_when(id, self);
        Some(id)
    }
}

impl IntoFernCondition for crate::signal::Prop<bool> {
    fn fern_into_conditional_child<W: crate::widget::Widget + 'static>(
        self,
        child: W,
        ctx: &mut crate::build_context::BuildContext,
    ) -> Option<WidgetId> {
        let id = ctx.add(child);
        ctx.visible_when(id, self);
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_widgets::FillWidget;
    use crate::widget_tree::WidgetTree;
    use fern_canvas::SizeProposal;
    use fern_tokens::Color;

    #[test]
    fn fern_branch_dispatches_to_active_variant() {
        // Build two trees, one with each variant, confirm each variant's
        // widget actually runs its own build/size/paint path.
        let mut tree_l = WidgetTree::new();
        let id_l = tree_l.add(FernBranch::<FillWidget, FillWidget>::L(
            FillWidget::new().background(Color::RED),
        ));
        tree_l.layout(SizeProposal::exact(100.0, 50.0));
        assert!((tree_l.bounds(id_l).width - 100.0).abs() < 0.01);

        let mut tree_r = WidgetTree::new();
        let id_r = tree_r.add(FernBranch::<FillWidget, FillWidget>::R(
            FillWidget::new().background(Color::BLUE),
        ));
        tree_r.layout(SizeProposal::exact(80.0, 40.0));
        assert!((tree_r.bounds(id_r).width - 80.0).abs() < 0.01);
    }

    #[test]
    fn fern_branch3_dispatches_to_active_variant() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FernBranch3::<FillWidget, FillWidget, FillWidget>::B(
            FillWidget::new(),
        ));
        tree.layout(SizeProposal::exact(120.0, 60.0));
        assert!((tree.bounds(id).width - 120.0).abs() < 0.01);
    }

    #[test]
    fn into_fern_child_routes_widget_to_deferred() {
        let pending = FillWidget::new().into_pending();
        assert!(matches!(pending, PendingChild::Deferred(_)));
    }

    #[test]
    fn into_fern_child_routes_widget_id_to_id() {
        let mut tree = WidgetTree::new();
        let leaf = tree.add(FillWidget::new());
        let pending = leaf.into_pending();
        match pending {
            PendingChild::Id(id) => assert_eq!(id, leaf),
            _ => panic!("expected PendingChild::Id"),
        }
    }
}
