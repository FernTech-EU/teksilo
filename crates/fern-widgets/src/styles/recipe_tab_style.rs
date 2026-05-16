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
//! `TabStyle` carries two methods. `make_body` wraps a single tab
//! header: a leaf [`TabBodyPainter`] (accent indicator + focus ring)
//! sits behind the label / leading / trailing slot composition in a
//! `ZStack`. `make_bar` wraps the whole strip: a [`TabBarChrome`]
//! container stacks an optional backdrop `RectWidget`, a
//! [`TabBarChromePainter`] leaf (content-pane separator +
//! drag-reorder drop indicator), and the bar content — sizing to the
//! content under the real proposal so its inner `Expand` fills the
//! bar. Neither painter has an intrinsic size; each fills the bounds
//! its parent hands it.

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::styles::{TabBarChromeConfig, TabBarOrientation, TabStyle, TabStyleConfig};
use fern_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::CornerRadius;

use crate::primitives::{HStack, RectWidget, ZStack};

// IntUI design tokens for Tab. Moved here in Step 7 of the styling
// refactor — the recipe + parent TabHeader own their own dimensions
// instead of reading from `theme.components.tab`.
pub const TAB_EDITOR_HEIGHT: f32 = 50.0;
pub const TAB_TOOL_WINDOW_HEIGHT: f32 = 28.0;
pub const TAB_PADDING_HORIZONTAL: f32 = 12.0;
pub const TAB_UNDERLINE_ACTIVE: f32 = 3.0;
pub const TAB_UNDERLINE_HOVER: f32 = 2.0;
pub const TAB_CLOSE_BUTTON_SIZE: f32 = 16.0;
/// Thickness of the drag-reorder drop-indicator line.
const DROP_INDICATOR_WIDTH: f32 = 2.0;

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

    fn make_bar(&self, cfg: &TabBarChromeConfig, ctx: &mut BuildContext) -> WidgetId {
        // Bar chrome z-order (back → front): optional backdrop fill,
        // the separator + drop-indicator leaf painter, then the bar
        // content. This mirrors the old `TabBar::paint` order, where
        // the widget painted backdrop/separator/indicator before its
        // children drew on top.
        //
        // A plain `ZStack` won't do here: it sizes itself by querying
        // children with an *unspecified* proposal, which collapses the
        // content's inner `Expand` to zero width. `TabBarChrome`
        // instead sizes to the content child under the *real*
        // proposal and places every layer at the full bar bounds.
        let painter = ctx.add(TabBarChromePainter {
            orientation: cfg.orientation,
            show_separator: cfg.show_separator,
            drop_indicator: cfg.drop_indicator.clone(),
        });

        let mut layers = Vec::with_capacity(3);
        if let Some(role) = &cfg.surface_role {
            // A `RectWidget` (not a painted fill in the leaf) so the
            // backdrop tracks `ColorProp` bindings — static role,
            // `Signal<Color>`, or `Signal<Role>` — correctly.
            let backdrop = ctx.add(RectWidget::new().background(role.clone()));
            layers.push(backdrop);
        }
        layers.push(painter);
        layers.push(cfg.content);

        ctx.add(TabBarChrome {
            layers,
            content: cfg.content,
        })
    }
}

/// Bar-chrome container produced by [`RecipeTabStyle::make_bar`].
/// Stacks the backdrop / chrome-painter / content layers at the full
/// bar bounds, but — unlike `ZStack` — sizes itself to the content
/// child under the real layout proposal so the content's inner
/// `Expand` fills the bar instead of collapsing to zero.
#[derive(Debug)]
struct TabBarChrome {
    /// Back-to-front: `[backdrop?, painter, content]`.
    layers: Vec<WidgetId>,
    /// The layer that drives the bar's size.
    content: WidgetId,
}

impl Widget for TabBarChrome {
    fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.layers.clone()
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
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.layers.clone()
    }
}

/// Internal leaf widget that paints the bar-level chrome at the
/// strip's bounds: the content-pane separator and the drag-reorder
/// drop indicator. The backdrop fill is a sibling `RectWidget`, not
/// painted here, so `ColorProp` bindings resolve correctly.
struct TabBarChromePainter {
    orientation: TabBarOrientation,
    show_separator: bool,
    drop_indicator: Signal<Option<f32>>,
}

impl std::fmt::Debug for TabBarChromePainter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabBarChromePainter")
            .field("orientation", &self.orientation)
            .field("show_separator", &self.show_separator)
            .finish()
    }
}

impl Widget for TabBarChromePainter {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.drop_indicator.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        // Leaf painter — no intrinsic size, so accept whatever the
        // parent proposes. ZStack/TabBarChrome propose the full bar
        // bounds, then use the returned size as the placement. If we
        // returned ZERO here, the painter would be placed at zero
        // bounds and the separator + drop indicator would never show.
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
        if self.show_separator {
            // 1 dp separator: bottom in horizontal mode, trailing
            // edge (right) in vertical mode. Painted *inside* the
            // focus-ring envelope reserved by each header so the
            // selected header's `surface_content` fill overpaints
            // the separator in its own column (the "tab merges into
            // content pane" effect).
            let border_width = ctx.theme.shape.border_width;
            let envelope = ctx.theme.shape.focus_ring_offset + ctx.theme.shape.focus_ring_width;
            let separator = match self.orientation {
                TabBarOrientation::Horizontal => Rect::new(
                    bounds.x,
                    (bounds.bottom() - envelope - border_width).max(bounds.y),
                    bounds.width,
                    border_width,
                ),
                TabBarOrientation::Vertical => Rect::new(
                    (bounds.right() - envelope - border_width).max(bounds.x),
                    bounds.y,
                    border_width,
                    bounds.height,
                ),
            };
            canvas.fill_rect(separator, ctx.theme.colors.border);
        }

        // Drop indicator: a vertical accent-color line at the
        // would-be insertion x in horizontal mode, a horizontal line
        // at the insertion y in vertical mode. The position is the
        // layout-axis offset stored in bar-local coords by the bar's
        // `on_drag_hover` handler.
        if let Some(local_pos) = self.drop_indicator.get() {
            let indicator = match self.orientation {
                TabBarOrientation::Horizontal => {
                    let world_x = bounds.x + local_pos;
                    Rect::new(
                        (world_x - DROP_INDICATOR_WIDTH * 0.5).max(bounds.x),
                        bounds.y,
                        DROP_INDICATOR_WIDTH,
                        bounds.height,
                    )
                }
                TabBarOrientation::Vertical => {
                    let world_y = bounds.y + local_pos;
                    Rect::new(
                        bounds.x,
                        (world_y - DROP_INDICATOR_WIDTH * 0.5).max(bounds.y),
                        bounds.width,
                        DROP_INDICATOR_WIDTH,
                    )
                }
            };
            canvas.fill_rect(indicator, ctx.theme.colors.accent);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational only — the parent TabBar carries Role::TabList.
        builder.set_hidden();
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

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        // Leaf painter — no intrinsic size, so accept whatever the
        // parent ZStack proposes. ZStack proposes the full tab
        // bounds, then uses the returned size as the placement. If we
        // returned ZERO here, the painter would be placed at zero
        // bounds and both the accent indicator and the focus ring
        // would never show.
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
