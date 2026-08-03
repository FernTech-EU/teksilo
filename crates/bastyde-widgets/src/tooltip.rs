// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tooltip system — hover-triggered overlays with configurable delay.
//!
//! Three tiers, increasing in expressive power:
//!
//! - [`TooltipWidget`] — single line of localized text in a themed
//!   rounded rect. Attached via the per-widget `.tooltip(...)` setter.
//! - [`RichTooltipWidget`] — `TooltipContent`-driven (body + optional
//!   long-form "more" disclosure + shortcut chip), inline-markup body
//!   so `[label](:key)` cascade links resolve against
//!   [`TooltipRegistry`]. Attached via `.rich_tooltip(key)` /
//!   `.rich_tooltip_content(content)`. On dwell it flips its AT role
//!   to `Role::Dialog` and advertises a `Focus` action — keyboard
//!   focus is not auto-transferred; the user Tabs in (the correct
//!   non-modal-panel a11y pattern).
//! - [`composite::CompositeTooltipWidget`] — hosts an arbitrary
//!   `impl Widget + 'static` body inside the same chrome with a
//!   larger surface budget. Crusader Kings 3-style: tabbed sections,
//!   charts, progress bars, conditional rows. Attached via
//!   `.composite_tooltip(content)`. "Primary-only" by construction —
//!   has no inline-markup body and no registry key, so it cannot be
//!   the target of a `[label](:key)` cascade. Child widgets *inside*
//!   the body keep their own tooltip setters and cascade normally.
//!
//! All three tiers share the same overlay machinery, hover/focus
//! tracking, and dwell-promotion timer in `bastyde-core`. The per-widget
//! setters (`.tooltip` / `.rich_tooltip` / `.composite_tooltip`) are
//! mutually exclusive (last-one-wins): each setter clears the others.
//!
//! ## Example — plain tooltip
//!
//! ```rust
//! # use bastyde_widgets::tooltip::TooltipWidget;
//! # use bastyde_i18n::lit;
//! let _tip = TooltipWidget::new(lit!("Save the current file"));
//! ```

pub mod attach;
pub mod composite;
pub(crate) mod dwell_indicator;
pub mod registry;
pub mod rich;

pub use attach::{
    RichTooltipSource, attach_composite_tooltip, attach_composite_tooltip_boxed,
    attach_composite_tooltip_boxed_with_placement, attach_rich_tooltip,
    attach_rich_tooltip_content, attach_rich_tooltip_content_with_placement,
    attach_rich_tooltip_source, attach_rich_tooltip_source_with_placement,
    attach_rich_tooltip_with_placement,
};
/// Where a tooltip opens relative to its anchor — re-exported from
/// `bastyde-core` so widgets can request `Side` placement in a vertical
/// list without naming the core path.
pub use bastyde_core::overlay::TooltipPlacement;
pub use composite::CompositeTooltipWidget;
pub use registry::{
    TooltipContent, TooltipRegistry, install_tooltip_registry, with_tooltip_registry,
};
pub use rich::RichTooltipWidget;

use bastyde_i18n::lit;
use std::rc::Rc;

use bastyde_canvas::{Canvas, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Prop;
use bastyde_core::styles::{SharedTooltipStyle, TooltipStyleConfig};
use bastyde_core::widget::{LayoutContext, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{CornerRadius, TextRole, TextStyleRole};

use crate::primitives::TextWidget;
use crate::shadow::paint_layered_shadow;
use bastyde_i18n::LocalizedString;

/// Tooltip-specific wrapper around [`paint_layered_shadow`] — pulls the
/// xs outer + inner shadow tokens and the per-component
/// `shadow_density` from the theme.
pub(crate) fn paint_tooltip_shadows(
    canvas: &mut Canvas,
    bounds: Rect,
    radius: CornerRadius,
    ctx: &PaintContext,
) {
    paint_layered_shadow(
        canvas,
        bounds,
        radius,
        &ctx.theme.shape.shadow_xs,
        &ctx.theme.shape.shadow_inner_xs,
        crate::styles::recipe_tooltip_style::TOOLTIP_SHADOW_DENSITY,
        None,
    );
}

/// Composite-tooltip variant of [`paint_tooltip_shadows`] — uses the
/// medium shadow tier (the larger CK3-style surface deserves more
/// presence than the punchy `xs` rim of plain tooltips).
pub(crate) fn paint_composite_tooltip_shadows(
    canvas: &mut Canvas,
    bounds: Rect,
    radius: CornerRadius,
    ctx: &PaintContext,
) {
    paint_layered_shadow(
        canvas,
        bounds,
        radius,
        &ctx.theme.shape.shadow_md,
        &ctx.theme.shape.shadow_inner_md,
        crate::styles::recipe_tooltip_style::COMPOSITE_TOOLTIP_SHADOW_DENSITY,
        None,
    );
}

/// A tooltip content widget — a themed rounded rect with text.
///
/// Composes a `TextWidget` with `Small` typography in `tooltip_text` color,
/// then delegates the chrome (shadow, dark background, corner radius,
/// padding) to the active `TooltipStyle` (default
/// [`crate::styles::RecipeTooltipStyle`]). Apps install per-call
/// (`TooltipWidget::new(...).style(impl TooltipStyle)`) or theme-wide
/// via `theme.style_slots.tooltip = Some(Rc::new(MyTooltip))`.
pub struct TooltipWidget {
    /// The tooltip body as a `Prop<String>`. A `tr!(...)` / `lit!(...)`
    /// source enters via [`TooltipWidget::new`] (locale-reactive when an
    /// `I18nManager` is installed); a `Signal<String>` source enters via
    /// [`TooltipWidget::bound`] for callers that swap the text at runtime
    /// (e.g. a single reusable tooltip surface reused across many
    /// hover targets, as `bastyde-scene` does for lightweight items).
    /// Either way the inner `TextWidget` re-renders on change without a
    /// rebuild.
    text: Prop<String>,
    style_override: Option<SharedTooltipStyle>,
    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for TooltipWidget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TooltipWidget")
            .field("text", &self.text.get())
            .finish()
    }
}

impl TooltipWidget {
    /// Construct a tooltip from a localized string. With an `I18nManager`
    /// installed the body stays locale-reactive (re-resolves on locale
    /// change); otherwise it's a static snapshot.
    pub fn new(text: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = text.into();
        Self {
            text: Prop::from(ls),
            style_override: None,
            root_child_id: None,
        }
    }

    /// Construct a tooltip whose body is driven by a `Signal<String>`
    /// (or any `Prop<String>`). Mutating the signal re-renders the
    /// tooltip in place — used when a single dormant tooltip surface is
    /// reused across many anchors and its text is set just before each
    /// show. Callers wanting locale reactivity should resolve their
    /// `LocalizedString` against the active locale when setting the
    /// signal.
    pub fn bound(text: impl Into<Prop<String>>) -> Self {
        Self {
            text: text.into(),
            style_override: None,
            root_child_id: None,
        }
    }

    /// Per-call style override. Replaces the theme-wide default
    /// `TooltipStyle` for just this TooltipWidget instance.
    pub fn style(mut self, style: impl bastyde_core::styles::TooltipStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }
}

impl Widget for TooltipWidget {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Wrap, don't ellipsize. `single_line()` only truncates against a
        // *bounded* width, and the overlay measures its content with an
        // unbounded proposal — so a long body used to render as one endless
        // line running off the window rather than as the capped, wrapped block
        // the `TOOLTIP_MAX_WIDTH` token describes. `layout_response` below
        // supplies the bound; `TextOverflow::Wrap` is the default.
        let text = TextWidget::new(lit!(""))
            .text(self.text.clone())
            .style(TextStyleRole::Small)
            .color(TextRole::TooltipText);
        let text_id = ctx.add(text);

        let style: SharedTooltipStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.tooltip.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeTooltipStyle::default()));
        let cfg = TooltipStyleConfig { content: text_id };
        let root_id = style.make_body(&cfg, ctx);
        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        // Clamp the proposal to the tooltip max-width token, mirroring
        // `RichTooltipWidget::layout_response`. The overlay content pass
        // measures with `width: None`, so without this the body has no width
        // to wrap against and the surface stretches to the full length of the
        // string.
        let max_w = crate::styles::recipe_tooltip_style::TOOLTIP_MAX_WIDTH;
        let clamped = SizeProposal {
            width: Some(proposal.width.map(|w| w.min(max_w)).unwrap_or(max_w)),
            height: proposal.height,
        };
        if let Some(root) = self.root_child_id
            && let Some(size) = ctx.child_size(root, clamped)
        {
            return size.into();
        }
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bastyde_canvas::Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::Tooltip);
        // Read the current value at walk time — the AT tree re-walks on a
        // locale change (Bound text) and on signal mutation (scene reuse),
        // so the announced name stays in sync with what's painted.
        builder.set_name(self.text.get());
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }

    /// A plain tooltip is entirely its string, so an empty or whitespace-only
    /// body — an unresolved i18n key, a `Signal<String>` not yet filled in —
    /// has nothing to show and must not open a blank bubble. Read at show
    /// time, so a bound tooltip that gains text later shows normally.
    fn tooltip_has_content(&self) -> bool {
        !self.text.get().trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_canvas::SizeProposal;
    use bastyde_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
    use bastyde_core::widget_tree::WidgetTree;

    /// A long single-word-free string that would run far past the cap if the
    /// body did not wrap.
    const LONG_BODY: &str = "This tooltip body is deliberately long enough that \
         it must wrap onto several lines instead of stretching the surface into \
         one endless ribbon that runs straight off the edge of the window.";

    /// Show a plain tooltip through the real overlay path (attach + hover +
    /// delay) and return the surface's laid-out bounds. The overlay content
    /// pass measures with an *unbounded* proposal, which is exactly the
    /// condition the wrapping fix has to survive — measuring the widget as a
    /// tree root instead would just hand it the root proposal.
    fn shown_tooltip_bounds(text: &str) -> bastyde_canvas::Rect {
        let mut tree = WidgetTree::new()
            .with_theme(bastyde_core::presets::intui::light())
            .with_text_backend(std::rc::Rc::new(std::cell::RefCell::new(
                bastyde_canvas::MockTextBackend::new(),
            )));
        let anchor = tree.add(crate::button::Button::new(lit!("Anchor")).tooltip(lit!(text)));
        tree.layout(SizeProposal::exact(2000.0, 600.0));
        tree.pointer_move(tree.bounds(anchor).center());
        tree.advance_time(std::time::Duration::from_secs(1));
        tree.layout(SizeProposal::exact(2000.0, 600.0));
        let overlay = *tree
            .active_overlays()
            .first()
            .expect("the tooltip is shown");
        tree.overlay_content_bounds(overlay)
            .expect("the shown overlay has content bounds")
    }

    #[test]
    fn a_long_plain_tooltip_wraps_at_the_max_width() {
        // Regression: the body was `single_line()` (ellipsis), which only
        // truncates against a *bounded* width — and the overlay measures its
        // content with `width: None`. So a long body rendered as one
        // unwrapped line running off the window, and TOOLTIP_MAX_WIDTH was
        // dead code for this tier (RichTooltipWidget clamps; plain did not).
        let long = shown_tooltip_bounds(LONG_BODY);
        assert!(
            long.width <= crate::styles::recipe_tooltip_style::TOOLTIP_MAX_WIDTH + 0.5,
            "a long tooltip must wrap at TOOLTIP_MAX_WIDTH, got {}",
            long.width
        );

        // ...and it wrapped rather than being truncated to one row.
        let short = shown_tooltip_bounds("short");
        assert!(
            long.height > short.height,
            "the wrapped body must occupy more than one line ({} vs {})",
            long.height,
            short.height
        );
    }

    #[test]
    fn an_empty_tooltip_has_no_content_to_show() {
        // A blank or unresolved string must not pop an empty chromed bubble.
        assert!(!TooltipWidget::new(lit!("")).tooltip_has_content());
        assert!(!TooltipWidget::new(lit!("   ")).tooltip_has_content());
        assert!(TooltipWidget::new(lit!("real")).tooltip_has_content());
    }

    #[test]
    fn an_empty_tooltip_never_opens_an_overlay() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let anchor = tree.add(crate::button::Button::new(lit!("Go")).tooltip(lit!("  ")));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        tree.pointer_move(tree.bounds(anchor).center());
        tree.advance_time(std::time::Duration::from_secs(1));
        assert!(
            tree.active_overlays().is_empty(),
            "a whitespace-only tooltip must not open an empty bubble"
        );
    }

    #[test]
    fn tooltip_widget_emits_shadow() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let _ = tree.add(TooltipWidget::new(lit!("hello")));
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let frame = tree.render();
        assert!(
            !frame.shadows.is_empty(),
            "tooltip should emit at least one shadow"
        );
    }

    #[test]
    fn tooltip_overlay_emits_shadow_through_fade() {
        // End-to-end-ish: anchor + tooltip overlay with a fade scope
        // applied (the production overlay path). Shadow must still
        // land in the rendered frame.
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let anchor = tree.add(TooltipWidget::new(lit!("anchor")));
        let tip = tree.add(TooltipWidget::new(lit!("hello")));
        tree.set_dormant(tip);
        tree.layout(SizeProposal::exact(800.0, 600.0));

        tree.show_overlay(OverlayRequest {
            content_id: tip,
            anchor,
            placement: OverlayPlacement::NearAnchor {
                offset: bastyde_canvas::Vec2::new(0.0, 8.0),
            },
            dismiss: DismissBehavior::PointerLeave {
                delay: std::time::Duration::from_millis(100),
            },
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: Some(std::time::Duration::from_millis(120)),
        });
        tree.layout(SizeProposal::exact(800.0, 600.0));
        let frame = tree.render();
        assert!(
            !frame.shadows.is_empty(),
            "tooltip overlay should emit at least one shadow even under fade scope"
        );
    }
}
