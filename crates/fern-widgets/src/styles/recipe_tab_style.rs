//! Default `TabStyle` impl driven by paint-recipe data.
//!
//! `RecipeTabStyle` ships the IntUI editor-tab look: an accent
//! indicator on the active tab's outside edge (top for horizontal
//! bars, leading for vertical bars), plus a keyboard focus ring
//! inset from the tab's bounds.
//!
//! The tab's own background (uniform across states, controlled by
//! `TabBar::tab_surface_role`) is painted by the surrounding
//! `TabHeader` as a separate RectWidget sibling — the trait config
//! doesn't carry the tab-surface role through the cfg, and pulling it
//! through every consumer would be more disruptive than letting the
//! widget keep that single rect.
//!
//! Layout: a leaf [`TabBodyPainter`] sits behind the label / leading /
//! trailing slot composition. Both are children of a ZStack with the
//! painter on the bottom. The painter has no intrinsic size — it
//! fills whatever bounds the parent gives it.

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::styles::{TabBarOrientation, TabStyle, TabStyleConfig};
use fern_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::CornerRadius;

use crate::primitives::{HStack, ZStack};

// IntUI design tokens for Tab. Moved here in Step 7 of the styling
// refactor — the recipe + parent TabHeader own their own dimensions
// instead of reading from `theme.components.tab`.
pub const TAB_EDITOR_HEIGHT: f32 = 50.0;
pub const TAB_TOOL_WINDOW_HEIGHT: f32 = 28.0;
pub const TAB_PADDING_HORIZONTAL: f32 = 12.0;
pub const TAB_UNDERLINE_ACTIVE: f32 = 3.0;
pub const TAB_UNDERLINE_HOVER: f32 = 2.0;
pub const TAB_CLOSE_BUTTON_SIZE: f32 = 16.0;

/// Default `TabStyle` shipped with FernUI.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeTabStyle;

impl TabStyle for RecipeTabStyle {
    fn make_body(&self, cfg: &TabStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        // Leaf painter for the chrome bits that live at the bounds
        // edges: accent indicator and focus ring.
        let painter = ctx.add(TabBodyPainter {
            is_active: cfg.is_active.clone(),
            is_focused: cfg.is_focused.clone(),
            is_disabled: cfg.is_disabled.clone(),
            orientation: cfg.orientation,
        });

        // Compose the slots. The widget today bundles everything into
        // `label` and passes None for leading/trailing — but custom
        // impls may use the three slots directly, so we honour them.
        let mut row = HStack::new();
        if let Some(id) = cfg.leading {
            row = row.add_child(id);
        }
        row = row.add_child(cfg.label);
        if let Some(id) = cfg.trailing {
            row = row.add_child(id);
        }
        let row_id = ctx.add(row);

        ctx.add(ZStack::new().add_child(painter).add_child(row_id))
    }
}

/// Internal leaf widget that paints the per-state chrome bits at the
/// edges of the tab's bounds. Not exposed publicly because custom
/// `TabStyle` impls compose their own body.
struct TabBodyPainter {
    is_active: Signal<bool>,
    is_focused: Signal<bool>,
    is_disabled: Signal<bool>,
    orientation: TabBarOrientation,
}

impl std::fmt::Debug for TabBodyPainter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabBodyPainter")
            .field("orientation", &self.orientation)
            .finish()
    }
}

impl Widget for TabBodyPainter {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.is_active
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.is_focused
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.is_disabled
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        vec![]
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        // Leaf painter has no intrinsic size; the ZStack hands it the
        // bounds the surrounding row reports.
        Size::ZERO.into()
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
        let shape = &ctx.theme.shape;
        let active = self.is_active.get();
        let focused = self.is_focused.get();
        let disabled = self.is_disabled.get();

        // Accent indicator on the layout-axis "outside" edge of the
        // selected, enabled tab. IntUI convention:
        //   - Horizontal bar → indicator on TOP (browser-tab look,
        //     selected tab "merges" into the content panel below).
        //   - Vertical bar → indicator on the LEADING edge (sidebar
        //     / IDE perspective look — the tab "points into" the
        //     content panel on the trailing side).
        let indicator_thickness = TAB_UNDERLINE_ACTIVE;
        if active && !disabled {
            let indicator = match self.orientation {
                TabBarOrientation::Horizontal => {
                    Rect::new(bounds.x, bounds.y, bounds.width, indicator_thickness)
                }
                TabBarOrientation::Vertical => {
                    Rect::new(bounds.x, bounds.y, indicator_thickness, bounds.height)
                }
            };
            canvas.fill_rect(indicator, colors.accent);
        }

        // Focus ring — keyboard focus only. Drawn inside `bounds`
        // (inset by `focus_ring_width / 2 + focus_ring_offset`) so
        // adjacent tabs aren't visually overlapped by the ring.
        if focused {
            let half_stroke = shape.focus_ring_width * 0.5;
            let inset = half_stroke + shape.focus_ring_offset;
            let ring_rect = Rect::new(
                bounds.x + inset,
                bounds.y + inset,
                (bounds.width - inset * 2.0).max(0.0),
                (bounds.height - inset * 2.0).max(0.0),
            );
            canvas.stroke_rounded_rect(
                ring_rect,
                CornerRadius::uniform(shape.radius_control),
                colors.focus_ring,
                shape.focus_ring_width,
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // The parent TabHeader carries the Role::Tab. This painter is
        // presentational only.
        builder.set_hidden();
    }
}
