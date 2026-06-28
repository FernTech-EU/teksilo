// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `SplitterStyle` impl — the IntUI divider-handle look.
//!
//! Reproduces the old `SplitView` chrome: a thin static line at the
//! gutter's center (so the divider never disappears), with a thicker
//! focus-color line that cross-fades in on hover-dwell and snaps to full
//! strength on keyboard focus or drag. The hit area is the full gutter
//! width; the cursor change is what signals grabbability.
//!
//! The visual body is a small private leaf (`SplitterHandleBody`) — same
//! "leaf body" choice as `RecipeSliderStyle`. Custom `SplitterStyle`
//! impls compose their own body instead.

use bastyde_canvas::{Canvas, Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::focus::FocusOrigin;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{SplitterStyle, SplitterStyleConfig};
use bastyde_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::Orientation;

/// Thickness of the always-present resting divider line, in dp.
pub const SPLITTER_DIVIDER_LINE_THICKNESS: f32 = 1.0;

/// Fraction of the hover-dwell animation spent fully transparent before
/// the focus line fades in (300 ms hold within the 400 ms dwell). Maps
/// the handle's linear `hover_progress` 0→1 onto a delayed alpha ramp.
const HOVER_DWELL_DELAY_FRAC: f32 = 0.75;

/// Configurable dimensions for [`RecipeSplitterStyle`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitterRecipe {
    /// Thickness of the always-present resting divider line, in dp.
    pub divider_line_thickness: f32,
}

impl Default for SplitterRecipe {
    fn default() -> Self {
        Self {
            divider_line_thickness: SPLITTER_DIVIDER_LINE_THICKNESS,
        }
    }
}

/// Default `SplitterStyle` shipped with Bastyde. Colors come from
/// `theme.colors.{border, focus_ring}`.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeSplitterStyle {
    pub recipe: SplitterRecipe,
}

impl RecipeSplitterStyle {
    pub fn new(recipe: SplitterRecipe) -> Self {
        Self { recipe }
    }
}

impl SplitterStyle for RecipeSplitterStyle {
    fn make_handle(&self, cfg: &SplitterStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        ctx.add(SplitterHandleBody {
            orientation: cfg.orientation,
            is_dragging: cfg.is_dragging.clone(),
            is_disabled: cfg.is_disabled.clone(),
            focus_origin: cfg.focus_origin.clone(),
            hover_progress: cfg.hover_progress.clone(),
            recipe: self.recipe,
        })
    }
}

/// Internal leaf that paints the divider line + focus indicator.
struct SplitterHandleBody {
    orientation: Orientation,
    is_dragging: Signal<bool>,
    is_disabled: Signal<bool>,
    focus_origin: Signal<Option<FocusOrigin>>,
    hover_progress: Signal<f32>,
    recipe: SplitterRecipe,
}

impl std::fmt::Debug for SplitterHandleBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SplitterHandleBody")
            .field("orientation", &self.orientation)
            .finish()
    }
}

impl Widget for SplitterHandleBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.is_dragging
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.is_disabled
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.focus_origin
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.hover_progress
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        // The host handle assigns exact bounds; just resolve the proposal.
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        _bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let colors = &ctx.theme.colors;
        let enabled = !self.is_disabled.get();

        let line_thickness = self.recipe.divider_line_thickness.max(1.0);
        let focus_thickness = (line_thickness * 3.0).max(line_thickness + 2.0);

        let line_rect = |thickness: f32| match self.orientation {
            // Horizontal splitter → vertical handle bar (line runs down).
            Orientation::Horizontal => Rect::new(
                bounds.x + (bounds.width - thickness) / 2.0,
                bounds.y,
                thickness,
                bounds.height,
            ),
            Orientation::Vertical => Rect::new(
                bounds.x,
                bounds.y + (bounds.height - thickness) / 2.0,
                bounds.width,
                thickness,
            ),
        };

        // Resting line — always present.
        canvas.fill_rect(line_rect(line_thickness), colors.border);

        // Focus indicator: instant on keyboard focus / drag, hover-dwell
        // fade-in otherwise.
        let focus_alpha = if !enabled {
            0.0
        } else if self.focus_origin.get() == Some(FocusOrigin::Keyboard) || self.is_dragging.get() {
            1.0
        } else {
            let p = self.hover_progress.get();
            ((p - HOVER_DWELL_DELAY_FRAC) / (1.0 - HOVER_DWELL_DELAY_FRAC)).clamp(0.0, 1.0)
        };

        if focus_alpha > 0.0 {
            canvas.fill_rect(
                line_rect(focus_thickness),
                colors.focus_ring.with_alpha(focus_alpha),
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational — the SplitterHandle owns the Role::Splitter node.
        builder.set_hidden();
    }
}
