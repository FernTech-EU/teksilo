// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Fluent list / tree row, with its selection **pill**.
//!
//! Windows 11 does not tint a selected row with the accent colour. It
//! gives it the same neutral `SubtleFill` wash a hovered row gets, and
//! marks it instead with a small accent bar on the leading edge — 3 × 16 dp
//! at a 1.5 dp radius, vertically centred
//! (`NavigationViewSelectionIndicator`,
//! `ListViewItemSelectionIndicatorCornerRadius`). That bar is the single
//! most recognisable element of a Fluent list, and it is what makes a
//! neutral selection wash legible.
//!
//! The row layout itself — slot gaps, subtitle sub-row, chevron column,
//! tree indent — is `StandardListItem`-specific and identical across design
//! languages, so it is delegated to [`RecipeStandardItemStyle`] with Fluent
//! metrics and the pill is stacked over the result.
//!
//! The delegate's own `selection_edge_width` is set to zero here: it exists
//! to give a pale selection wash a non-colour-alone boundary (WCAG 1.4.1 /
//! 1.4.11), and the pill discharges that duty with a shape cue Fluent
//! actually has. The keyboard focus ring is left at Fluent's
//! `FocusVisualPrimaryThickness`.

use teksilo_canvas::{Canvas, Point, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::environment::LayoutDirection;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{StandardItemStyle, StandardItemStyleConfig};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::CornerRadius;
use teksilo_widgets::styles::{RecipeStandardItemStyle, StandardItemRecipe};

use crate::shape::{FLUENT_CONTROL_CORNER_RADIUS, FLUENT_FOCUS_RING_WIDTH};

/// `NavigationViewSelectionIndicator` width (dp).
const PILL_WIDTH: f32 = 3.0;
/// `NavigationViewSelectionIndicator` height (dp).
const PILL_HEIGHT: f32 = 16.0;
/// `ListViewItemSelectionIndicatorCornerRadius` (dp).
const PILL_RADIUS: f32 = 1.5;
/// `ListViewItemMinHeight` (dp).
const ROW_HEIGHT: f32 = 40.0;
/// A row carrying a subtitle: the single-line height plus a Caption line box.
const ROW_HEIGHT_TWO_LINE: f32 = 60.0;
/// `NavigationViewItemOnLeftIconBoxHeight` (dp).
const ICON_SIZE: f32 = 16.0;

// The pill has to leave breathing room inside the row it marks.
const _: () = assert!(PILL_HEIGHT < ROW_HEIGHT);
const _: () = assert!(ROW_HEIGHT < ROW_HEIGHT_TWO_LINE);

/// The Fluent [`StandardItemRecipe`] — public so an app can tune one
/// dimension without rebuilding the style.
pub fn fluent_standard_item_recipe() -> StandardItemRecipe {
    StandardItemRecipe {
        icon_size: ICON_SIZE,
        padding_horizontal: 12.0,
        min_height_single_line: ROW_HEIGHT,
        min_height_two_line: ROW_HEIGHT_TWO_LINE,
        tree_indent_step: 16.0,
        item_corner_radius: FLUENT_CONTROL_CORNER_RADIUS,
        bg_horizontal_inset: 4.0,
        focus_ring_width: FLUENT_FOCUS_RING_WIDTH,
        // The pill replaces the always-on selection outline; see the
        // module doc.
        selection_edge_width: 0.0,
        ..StandardItemRecipe::default()
    }
}

/// Fluent `StandardItemStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct FluentStandardItemStyle;

impl StandardItemStyle for FluentStandardItemStyle {
    fn make_body(&self, cfg: &StandardItemStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let row = RecipeStandardItemStyle::new(fluent_standard_item_recipe()).make_body(cfg, ctx);
        let pill = ctx.add(FluentSelectionPill {
            is_selected: cfg.is_selected.clone(),
            is_disabled: cfg.is_disabled.clone(),
        });
        ctx.add(FluentRowFrame { row, pill })
    }
}

/// Stacks the selection pill over the delegated row, stretching **both** to
/// the frame's bounds.
///
/// The obvious composition — a `ZStack` — is wrong here, and silently so.
/// `ZStack::layout_response` measures its children at an *unspecified* width
/// on purpose (so a greedy background cannot inflate the stack and a
/// shrinkable label cannot truncate during intrinsic measurement), then
/// `place_children` honours whatever width each child reported. The
/// delegated row's content sits behind an `Expand`, whose basis is zero, so
/// its natural width is just the padding — a `ZStack` wrapper would centre a
/// 24 dp row inside a 1200 dp list and spill its label straight out the
/// side. `StandardListItem` avoids this by placing its single root child at
/// full bounds; this frame does the same for two.
struct FluentRowFrame {
    row: WidgetId,
    pill: WidgetId,
}

impl std::fmt::Debug for FluentRowFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FluentRowFrame").finish()
    }
}

impl Widget for FluentRowFrame {
    fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> {
        vec![self.row, self.pill]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        // The row owns the frame's size — including its shrink weight and
        // compression floor, so a row inside a narrowing container still
        // truncates rather than overflowing. The pill is pure overlay.
        ctx.child_layout_response(self.row, proposal)
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into())
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

    fn children(&self) -> Vec<WidgetId> {
        vec![self.row, self.pill]
    }
}

/// Paints the leading accent bar of a selected row.
///
/// A leaf rather than a `Center`/`FixedSize`/`Spacer` composition: a
/// virtualized list realizes and recycles these constantly, and one node
/// per row beats five. RTL is resolved from
/// [`PaintContext::layout_direction`], so the bar follows the leading edge
/// rather than the left one.
struct FluentSelectionPill {
    is_selected: Signal<bool>,
    is_disabled: Signal<bool>,
}

impl std::fmt::Debug for FluentSelectionPill {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FluentSelectionPill").finish()
    }
}

impl Widget for FluentSelectionPill {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.is_selected
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.is_disabled
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
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
        if !self.is_selected.get() || self.is_disabled.get() {
            return;
        }
        let h = PILL_HEIGHT.min(bounds.height);
        let x = match ctx.layout_direction {
            LayoutDirection::RightToLeft => bounds.x + bounds.width - PILL_WIDTH,
            LayoutDirection::LeftToRight => bounds.x,
        };
        canvas.fill_rounded_rect(
            Rect::new(x, bounds.y + (bounds.height - h) * 0.5, PILL_WIDTH, h),
            CornerRadius::uniform(PILL_RADIUS),
            // Already desaturated by the paint walker in an inactive window.
            ctx.theme.colors.accent,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Purely decorative — selection is announced by the row itself.
        builder.set_hidden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pill_is_the_navigation_view_indicator() {
        // That it fits inside the row is a compile-time invariant above.
        assert_eq!(PILL_WIDTH, 3.0);
        assert_eq!(PILL_HEIGHT, 16.0);
        assert_eq!(PILL_RADIUS, 1.5);
        assert_eq!(ICON_SIZE, 16.0);
    }

    #[test]
    fn recipe_uses_fluent_row_metrics() {
        let r = fluent_standard_item_recipe();
        assert_eq!(r.min_height_single_line, 40.0);
        assert_eq!(r.item_corner_radius, FLUENT_CONTROL_CORNER_RADIUS);
        assert_eq!(r.focus_ring_width, FLUENT_FOCUS_RING_WIDTH);
        assert!(r.min_height_two_line > r.min_height_single_line);
    }

    #[test]
    fn selection_edge_is_handed_over_to_the_pill() {
        assert_eq!(fluent_standard_item_recipe().selection_edge_width, 0.0);
        // …and the keyboard focus ring is *not* — the two cues are
        // different affordances and both must survive.
        assert!(fluent_standard_item_recipe().focus_ring_width > 0.0);
    }

    #[test]
    fn rows_are_taller_than_the_intui_default() {
        assert!(
            fluent_standard_item_recipe().min_height_single_line
                > StandardItemRecipe::default().min_height_single_line
        );
    }
}
