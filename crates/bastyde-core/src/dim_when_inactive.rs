// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `DimWhenInactive` — a wrapper widget that dims its subtree when the host
//! window loses active status.
//!
//! This is the per-widget **opt-in** layer of the window-active appearance
//! model (the analogue of SwiftUI's `@Environment(\.appearsActive)` read or
//! GTK's `:backdrop`-driven custom styling). The automatic layers — caret
//! hiding and selection desaturation in stock widgets — need no wrapping; this
//! is for *custom content* an app wants to fade back when its window isn't the
//! active one (a colourful side panel, a bespoke accent surface, a banner).
//!
//! It reads [`BuildContext::window_active_signal`] and drives an
//! `opacity: Signal<f32>` (1.0 when active, `factor` when inactive) onto its
//! own subtree via [`BuildContext::set_opacity`]. The render walker emits the
//! matching `SetOpacity`/`RestoreOpacity` pair, so the multiplier composes with
//! any ancestor opacity scope.
//!
//! ```ignore
//! use bastyde_core::widget_builder::WidgetBuilder;
//! // Fade a custom panel to 40 % when the window is inactive:
//! ctx.add(my_panel.dim_when_inactive(0.4));
//! // Or directly:
//! ctx.add(DimWhenInactive::new().child(my_panel).factor(0.4));
//! ```
//!
//! ## Layout & a11y semantics
//!
//! Layout-transparent: the wrapped child reports its full natural size at every
//! opacity, so dimming never drives layout jitter. The wrapper is also
//! a11y-transparent — the child owns its own semantics. The opacity **snaps**
//! (no tween) on the active flip, which is already correct under
//! `prefers-reduced-motion`: window activation is an OS state change, not a
//! user-initiated motion.

use bastyde_canvas::{Point, Rect, SizeProposal};

use crate::accessibility::AccessNodeBuilder;
use crate::build_context::BuildContext;
use crate::widget::{LayoutContext, PendingChild, Widget, WidgetPlacement};
use crate::widget_id::WidgetId;

/// Default dim factor: the subtree drops to 70 % opacity when the window is
/// inactive — perceptible but not distracting.
pub const DEFAULT_DIM_FACTOR: f32 = 0.7;

/// Wraps a child and dims the whole subtree (multiplies its opacity by
/// `factor`) whenever the host window is not active. See the module docs.
pub struct DimWhenInactive {
    pending_child: Option<PendingChild>,
    child_id: Option<WidgetId>,
    factor: f32,
}

impl DimWhenInactive {
    /// New dim wrapper with the [`DEFAULT_DIM_FACTOR`]. Attach a child with
    /// [`child`](Self::child) / [`child_id`](Self::child_id).
    pub fn new() -> Self {
        Self {
            pending_child: None,
            child_id: None,
            factor: DEFAULT_DIM_FACTOR,
        }
    }

    /// Inline child widget (deferred insertion).
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_child = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// Pre-registered child by `WidgetId`.
    pub fn child_id(mut self, id: WidgetId) -> Self {
        self.pending_child = Some(PendingChild::Id(id));
        self
    }

    /// Opacity applied while the window is inactive (clamped to `0.0..=1.0`).
    /// `1.0` is a no-op; `0.0` fully hides the subtree. Default
    /// [`DEFAULT_DIM_FACTOR`].
    pub fn factor(mut self, factor: f32) -> Self {
        self.factor = factor.clamp(0.0, 1.0);
        self
    }
}

impl Default for DimWhenInactive {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for DimWhenInactive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DimWhenInactive")
            .field("factor", &self.factor)
            .finish()
    }
}

impl Widget for DimWhenInactive {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        let Some(child_id) = self.child_id else {
            return vec![];
        };

        // Derive opacity from the window-active signal: full when active,
        // `factor` when not. The derived signal's upstream (window_active) is
        // registered by `set_opacity` at RepaintOnly, so a focus flip repaints
        // this subtree with the new multiplier (no relayout).
        let factor = self.factor;
        let opacity = ctx
            .window_active_signal()
            .map(move |&active| if active { 1.0 } else { factor });

        let id = ctx.self_id();
        ctx.set_opacity(id, opacity);

        vec![child_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> crate::widget::LayoutResponse {
        // Layout-transparent: report the child's natural size at all opacities.
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
        // Visual-modulation wrapper only — the wrapped subtree owns its a11y.
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_widgets::FillWidget;
    use crate::widget_tree::WidgetTree;
    use bastyde_canvas::{DrawCommand, SizeProposal};
    use bastyde_tokens::Color;

    fn set_opacities(frame: &bastyde_canvas::RenderFrame) -> Vec<f32> {
        frame
            .draw_order
            .iter()
            .filter_map(|c| match c {
                DrawCommand::SetOpacity(v) => Some(*v),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn window_active_defaults_true() {
        // A window must not be born inactive (winit may not send Focused(true)
        // for the first window).
        let tree = WidgetTree::new();
        assert!(tree.is_window_active());
        assert!(tree.window_active_signal().get());
    }

    #[test]
    fn window_active_state_is_per_tree() {
        // Each window owns its own state — deactivating one must not touch
        // another (no app-wide fan-out, unlike theme / text-scale).
        let mut a = WidgetTree::new();
        let b = WidgetTree::new();
        a.set_window_active(false);
        assert!(!a.is_window_active(), "tree A is inactive");
        assert!(b.is_window_active(), "tree B is unaffected");
    }

    #[test]
    fn factor_is_clamped() {
        assert_eq!(DimWhenInactive::new().factor(2.0).factor, 1.0);
        assert_eq!(DimWhenInactive::new().factor(-1.0).factor, 0.0);
        assert_eq!(DimWhenInactive::new().factor, DEFAULT_DIM_FACTOR);
    }

    #[test]
    fn dims_subtree_only_when_window_inactive() {
        let mut tree = WidgetTree::new().with_theme(crate::presets::intui::light());
        tree.add(
            DimWhenInactive::new()
                .factor(0.5)
                .child(FillWidget::new().background(Color::RED)),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));

        // Active: no dimming scope (< 1.0) is emitted.
        let ops = set_opacities(&tree.render());
        assert!(
            !ops.iter().any(|o| *o < 0.99),
            "active window must not dim, got {ops:?}"
        );

        // Inactive: a 0.5 opacity scope wraps the subtree.
        tree.set_window_active(false);
        let ops = set_opacities(&tree.render());
        assert!(
            ops.iter().any(|o| (*o - 0.5).abs() < 1e-3),
            "inactive window must dim to the factor, got {ops:?}"
        );

        // Reactivate: dimming clears.
        tree.set_window_active(true);
        let ops = set_opacities(&tree.render());
        assert!(
            !ops.iter().any(|o| *o < 0.99),
            "reactivated window must not dim, got {ops:?}"
        );
    }
}
