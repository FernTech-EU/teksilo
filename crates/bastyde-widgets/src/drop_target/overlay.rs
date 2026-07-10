// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `DropRegionOverlay` — the paint-and-place layer for a multi-zone
//! [`DropTarget`](super::DropTarget).
//!
//! A generalization of docking's `DockDropOverlay`: a decorative, pointer-
//! transparent widget stacked over the wrapped content that (a) paints the
//! **active region's** affordance and (b) hosts the per-region hint cards,
//! placing each centered within its own region rect. It reads a shared
//! `Signal<Option<DropRegion>>` (written by the `DropTarget`'s hover handler)
//! and repaints on change.
//!
//! Painting rules (honouring "the wrapped content stays visible"):
//! - a **side** zone → a translucent fill (~0.22 alpha) **plus** an accent frame —
//!   an edge strip fill reads clearly as "drop here".
//! - `Center` → painted **by the recipe's full-bounds rounded border**, not here
//!   (so the single-zone accept keeps its rounded corners); the overlay skips it.
//!
//! The overlay itself is a transparent container (`Role::GenericContainer`), not
//! AT-hidden, so the hint cards it hosts keep their `Live::Polite` announcement
//! when their region becomes active. The hint cards are gated with
//! `visible_when` by the recipe, so a non-active zone's hint leaves paint + AT
//! entirely.

use bastyde_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::accesskit::Role;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{DropRegion, region_rect};
use bastyde_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{BorderRole, SurfaceRole};

/// Translucent fill alpha for a side-zone highlight.
const REGION_FILL_ALPHA: f32 = 0.22;

/// Paint + placement layer for a multi-zone drop target. Added as the topmost
/// `ZStack` child by `RecipeDropTargetStyle::make_body`.
pub(crate) struct DropRegionOverlay {
    /// Which region is the active accepted-hover (drives paint + which hint shows).
    active_region: Signal<Option<DropRegion>>,
    /// Side-zone size factor (already clamped to `0.1..=1.0`).
    size_factor: f32,
    /// Frame thickness for the highlight (0 = paint nothing — `variant == None`).
    border_width: f32,
    /// Per-region hint cards this overlay hosts and places. Order matches the
    /// `children()` / `build()` return order.
    hints: Vec<(DropRegion, WidgetId)>,
    fill: ColorProp,
    border: ColorProp,
}

impl std::fmt::Debug for DropRegionOverlay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DropRegionOverlay")
            .field("size_factor", &self.size_factor)
            .field("border_width", &self.border_width)
            .field("hint_count", &self.hints.len())
            .finish()
    }
}

impl DropRegionOverlay {
    pub(crate) fn new(
        active_region: Signal<Option<DropRegion>>,
        size_factor: f32,
        border_width: f32,
        hints: Vec<(DropRegion, WidgetId)>,
    ) -> Self {
        Self {
            active_region,
            size_factor,
            border_width,
            hints,
            fill: ColorProp::from(SurfaceRole::Accent),
            border: ColorProp::from(BorderRole::Accent),
        }
    }
}

impl Widget for DropRegionOverlay {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Repaint (not relayout) when the active region changes.
        self.active_region.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        // Pure decoration: pointer events fall through to the content below.
        ctx.apply_self_handlers(HandlerSet::new().event_pass_through(true));
        // Host the hint cards so `place_children` can position them per region.
        self.hints.iter().map(|(_, id)| *id).collect()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        // Absorbed into the ZStack's shared bounds — never inflates it.
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            // The child order matches `self.hints`; map its id back to a region.
            let region = self
                .hints
                .iter()
                .find(|(_, id)| *id == child.id)
                .map(|(r, _)| *r)
                .unwrap_or(DropRegion::Center);
            let zone = region_rect(region, bounds, self.size_factor);
            let natural = ctx
                .child_size(child.id, SizeProposal::unspecified())
                .unwrap_or(zone.size());
            // Clamp the hint to its zone so an oversized hint can't bleed into a
            // neighbouring zone; the DropTarget's `clips_children` hides any
            // truncated remainder. Then centre it within the zone rect.
            let child_size = Size::new(
                natural.width.min(zone.width),
                natural.height.min(zone.height),
            );
            let dx = ((zone.width - child_size.width) / 2.0).max(0.0);
            let dy = ((zone.height - child_size.height) / 2.0).max(0.0);
            child.origin = Point::new(zone.x + dx, zone.y + dy);
            child.size = child_size;
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        if self.border_width <= 0.0 {
            return;
        }
        let Some(region) = self.active_region.get() else {
            return;
        };
        // `Center` is drawn by the recipe's full-bounds rounded border (so the
        // single-zone accept keeps its rounded corners); the overlay only paints
        // the four side zones.
        if !region.is_side() {
            return;
        }
        let rect = region_rect(region, bounds, self.size_factor);
        // Side zones get a translucent fill + accent frame.
        let fill = self
            .fill
            .resolve(ctx.theme, true)
            .with_alpha(REGION_FILL_ALPHA);
        canvas.fill_rect(rect, fill);
        let border = self.border.resolve(ctx.theme, true);
        let t = self.border_width;
        canvas.fill_rect(Rect::new(rect.x, rect.y, rect.width, t), border);
        canvas.fill_rect(
            Rect::new(rect.x, rect.y + rect.height - t, rect.width, t),
            border,
        );
        canvas.fill_rect(Rect::new(rect.x, rect.y, t, rect.height), border);
        canvas.fill_rect(
            Rect::new(rect.x + rect.width - t, rect.y, t, rect.height),
            border,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        if self.hints.is_empty() {
            // Pure decoration (no hosted hint cards) — hide it from AT so a
            // hint-less multi-zone target (e.g. a docking pane) doesn't add an
            // empty container per drop target. Matches the old hand-rolled
            // docking overlay, which was `set_hidden()`.
            builder.set_hidden();
        } else {
            // Hosts hint cards: stay a transparent container so their
            // `Live::Polite` announcement survives (a hidden node prunes its
            // subtree from AT).
            builder.set_role(Role::GenericContainer);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.hints.iter().map(|(_, id)| *id).collect()
    }
}
