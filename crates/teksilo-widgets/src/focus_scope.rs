// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `FocusScope` — a layout-transparent wrapper that declares a **traversal
//! boundary** for Tab / Shift+Tab focus cycling.
//!
//! Descendants' `tab_index` values are scoped to the nearest enclosing
//! `FocusScope`: two sibling scopes that both number their children `1, 2, 3`
//! never interleave — each scope is an independent, ordered unit within its
//! parent. The [`TraversalScopePolicy`] controls what Tab does at the scope's
//! ends:
//!
//! - [`Continue`](TraversalScopePolicy::Continue) — Tab flows *out* of the
//!   scope into the enclosing scope's next member (grouping only). Use for
//!   logical regions in a continuous Tab order, e.g. dock panels.
//! - [`Cycle`](TraversalScopePolicy::Cycle) — Tab *wraps* within the scope and
//!   never leaves via keyboard. Use for modal dialogs.
//!
//! ```ignore
//! // A modal dialog whose Tab order is confined to its own content:
//! FocusScope::new(TraversalScopePolicy::Cycle).child(dialog_body)
//! ```
//!
//! **Do not `Cycle`-wrap a popover, menu or dropdown panel.** Those are
//! non-modal, and the framework dismisses a non-modal overlay when keyboard
//! focus leaves it — which is what their ARIA patterns (Disclosure, Menu) ask
//! for, and what keeps an open panel from sitting over the focus ring that
//! left it. Trapping focus inside one prevents that dismissal from ever
//! firing. A centered modal needs no wrapper at all: `cycle_focus` already
//! roots traversal at the topmost centered overlay's content.
//!
//! ## Layout & accessibility
//!
//! `FocusScope` imposes no layout — it reports its child's natural size and
//! places the child at its own bounds (like [`Fade`](crate::Fade)). It is a
//! structural boundary, not an AT element: the wrapped child owns its own
//! accessibility semantics. The scope node is never itself a Tab stop
//! (`BuildContext::set_traversal_scope` forces it non-focusable).

use teksilo_canvas::{Point, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::focus::TraversalScopePolicy;
use teksilo_core::widget::{LayoutContext, LayoutResponse, PendingChild, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;

/// Wraps a child subtree and declares it a Tab traversal scope. See the
/// [module documentation](self) for semantics.
pub struct FocusScope {
    policy: TraversalScopePolicy,
    pending_child: Option<PendingChild>,
    child_id: Option<WidgetId>,
}

impl FocusScope {
    /// Create a traversal scope with the given boundary `policy`.
    pub fn new(policy: TraversalScopePolicy) -> Self {
        Self {
            policy,
            pending_child: None,
            child_id: None,
        }
    }

    /// Inline child widget (deferred insertion — the form `teksu!` lowers to).
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_child = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// Pre-registered child by `WidgetId`.
    pub fn child_id(mut self, id: WidgetId) -> Self {
        self.pending_child = Some(PendingChild::Id(id));
        self
    }
}

impl std::fmt::Debug for FocusScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusScope")
            .field("policy", &self.policy)
            .finish()
    }
}

impl Widget for FocusScope {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        // Mark this node as a traversal-scope boundary. This also forces the
        // node non-focusable: a scope is a boundary, never itself a Tab stop.
        ctx.set_traversal_scope(self.policy);
        self.child_id.into_iter().collect()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        // Layout-transparent: report the child's natural size unchanged.
        self.child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = Point::new(bounds.x, bounds.y);
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // Structural boundary only — the wrapped subtree owns its semantics.
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::TextWidget;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_i18n::lit;

    #[test]
    fn marks_node_with_its_policy() {
        let mut tree = WidgetTree::new();
        let scope = tree
            .add(FocusScope::new(TraversalScopePolicy::Cycle).child(TextWidget::new(lit!("x"))));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        assert_eq!(
            tree.traversal_scope(scope),
            Some(TraversalScopePolicy::Cycle),
            "FocusScope::build must install its policy on its own node"
        );
    }

    #[test]
    fn is_layout_transparent() {
        // The wrapper's bounds match the bare child's — it imposes no layout.
        let mut tree = WidgetTree::new();
        let bare = tree.add(TextWidget::new(lit!("hello")));
        let scoped = tree.add(
            FocusScope::new(TraversalScopePolicy::Continue).child(TextWidget::new(lit!("hello"))),
        );
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        assert_eq!(
            tree.bounds(bare).size(),
            tree.bounds(scoped).size(),
            "FocusScope must report its child's natural size"
        );
    }
}
