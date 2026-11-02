// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The `DockingLayout` centre slot: fills the centre rectangle and clips its
//! content, with room left past the edge for a focus ring.
//!
//! A clip at the centre's own bounds would cut the ring of a control flush
//! with the centre's edge: the Fluent and macOS presets, like IntUI, paint
//! their rings outside the control, from the control's own paint, so an
//! ancestor's clip reaches them. The framework clip is always a node's
//! bounds, so the slot is two nodes:
//!
//! - [`CentreFrame`] sits at the centre rectangle (what `DockingLayout`
//!   places and measures) and places its one child, the clip, at that
//!   rectangle grown by [`focus_ring_margin`];
//! - [`CentreClip`] clips at its own, grown bounds and lays the content
//!   back at the centre rectangle.
//!
//! The band past the edge is never hit: the hit test stops at the frame's
//! bounds, and the sides, painted after the centre, keep their presses.
//!
//! The same band lets content that overflows the centre paint up to the
//! margin (about 6 dp with IntUI) past the centre's edge: into a side's
//! resize-handle gutter, which paints only its divider line over it, or,
//! where a side is hidden or disabled, past the `DockingLayout`'s own edge,
//! over whatever lies there. Content cannot reach further than that.
//!
//! The margin follows the theme the frame is laid out with, so a runtime
//! theme change re-places the clip on the next layout; a theme scoped to
//! the centre's content alone is not seen.

use teksilo_canvas::{Rect, Size, SizeProposal};
use teksilo_core::build_context::BuildContext;
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::ShapeTokens;

/// How far past the centre's edge its clip reaches: the theme's focus-ring
/// envelope plus one ring width. The tokens describe a preset's main ring;
/// macOS draws a translucent halo one ring width outside it, which the
/// envelope alone would cut.
pub(super) fn focus_ring_margin(shape: &ShapeTokens) -> f32 {
    shape.focus_ring_envelope() + shape.focus_ring_width.max(0.0)
}

fn inflate(r: Rect, d: f32) -> Rect {
    Rect::new(r.x - d, r.y - d, r.width + d * 2.0, r.height + d * 2.0)
}

/// The centre rectangle. Answers a proposal like a bare `Expand`: it takes
/// what a bounded axis offers and asks nothing of an unbounded one.
#[derive(Debug)]
pub(super) struct CentreFrame {
    content: WidgetId,
    clip: Option<WidgetId>,
}

impl CentreFrame {
    pub(super) fn new(content: WidgetId) -> Self {
        Self {
            content,
            clip: None,
        }
    }
}

impl Widget for CentreFrame {
    // Like the `Expand` this replaces, the frame keeps the default
    // `preserves_children_on_rebuild` (false). Nothing rebuilds it: it binds
    // no signal, and `DockingLayout` re-attaches its centre by id instead of
    // building it again. A rebuild would destroy the clip and the content,
    // as it would have destroyed the `Expand`'s child.
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let clip = match self.clip {
            Some(id) => id,
            None => ctx.add(CentreClip {
                content: self.content,
            }),
        };
        self.clip = Some(clip);
        vec![clip]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        LayoutResponse::flexible(
            Size::new(
                proposal.width.unwrap_or(0.0),
                proposal.height.unwrap_or(0.0),
            ),
            1.0,
        )
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let grown = inflate(bounds, focus_ring_margin(&ctx.theme.shape));
        for child in children.iter_mut() {
            child.origin = grown.origin();
            child.size = grown.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.clip.into_iter().collect()
    }
}

/// Clips at its own bounds (the centre grown by the focus-ring margin) and
/// lays the content at the centre rectangle inside them.
#[derive(Debug)]
pub(super) struct CentreClip {
    content: WidgetId,
}

impl Widget for CentreClip {
    fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> {
        vec![self.content]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        ctx.child_size(self.content, proposal)
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let m = focus_ring_margin(&ctx.theme.shape);
        let inner = Rect::new(
            bounds.x + m,
            bounds.y + m,
            (bounds.width - 2.0 * m).max(0.0),
            (bounds.height - 2.0 * m).max(0.0),
        );
        for child in children.iter_mut() {
            child.origin = inner.origin();
            child.size = inner.size();
        }
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.content]
    }
}
