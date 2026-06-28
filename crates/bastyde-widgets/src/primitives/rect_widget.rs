// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! RectWidget — a leaf widget that paints a filled and/or stroked rounded rectangle.
//!
//! `RectWidget` has no intrinsic content: it fills whatever space its parent
//! proposes (or reports `0×0` when unconstrained) and draws a solid or reactive
//! background color, an optional border, and an optional corner radius. It is
//! the low-level building block for card backgrounds, focus rings, dividers, and
//! highlight overlays.
//!
//! All visual properties accept `impl Into<ColorProp>` (a raw `Color`, a theme
//! role such as `SurfaceRole::Hover`, or a `Signal<Color>`) so reactive
//! interaction-driven colors require no extra wiring.
//!
//! ```rust
//! # use bastyde_tokens::{Color, CornerRadius};
//! # use bastyde_widgets::primitives::RectWidget;
//! // A pill-shaped accent badge background:
//! let _w = RectWidget::new()
//!     .background(Color::from_rgba(0.2, 0.5, 1.0, 1.0))
//!     .corner_radius(CornerRadius::uniform(12.0));
//! ```

use bastyde_canvas::{Canvas, Rect, SizeProposal};
use bastyde_tokens::{Color, CornerRadius};

use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::signal::Prop;
use bastyde_core::widget::{LayoutContext, PaintContext, Widget};

/// A leaf widget that paints a filled and/or stroked rounded rectangle.
///
/// See the [module documentation](self) for the full feature description.
/// All visual properties accept `impl Into<ColorProp>` (colors/roles/signals) or
/// `impl Into<Prop<f32>>` / `impl Into<Prop<CornerRadius>>` (static or reactive)
/// — so the common "fill with theme surface, border with theme border" setup is
/// just `.background(SurfaceRole::Main).border_color(BorderRole::Default)`.
pub struct RectWidget {
    background: ColorProp,
    border_color: ColorProp,
    border_width: Prop<f32>,
    corner_radius: Prop<CornerRadius>,
}

impl RectWidget {
    /// Create a fully transparent, zero-border rectangle with no corner radius.
    pub fn new() -> Self {
        Self {
            background: ColorProp::Static(Color::TRANSPARENT),
            border_color: ColorProp::Static(Color::TRANSPARENT),
            border_width: Prop::Static(0.0),
            corner_radius: Prop::Static(CornerRadius::ZERO),
        }
    }

    /// Fill color. Accepts `Color`, a theme role (`SurfaceRole`, etc.),
    /// or a `Signal<Color>`.
    pub fn background(mut self, color: impl Into<ColorProp>) -> Self {
        self.background = color.into();
        self
    }

    /// Border color. Accepts `Color`, a theme role (`BorderRole`, etc.),
    /// or a `Signal<Color>`.
    pub fn border_color(mut self, color: impl Into<ColorProp>) -> Self {
        self.border_color = color.into();
        self
    }

    /// Stroke width, in logical pixels. Accepts a static value or a reactive `Signal<f32>`.
    pub fn border_width(mut self, width: impl Into<Prop<f32>>) -> Self {
        self.border_width = width.into();
        self
    }

    /// Corner radius for the fill and stroke. Accepts a `CornerRadius` (per-corner
    /// control) or a reactive `Signal<CornerRadius>`.
    pub fn corner_radius(mut self, radius: impl Into<Prop<CornerRadius>>) -> Self {
        self.corner_radius = radius.into();
        self
    }

    /// Compatibility shim — `.bind_background(signal)` → `.background(signal)`.
    pub fn bind_background(self, state: impl Into<ColorProp>) -> Self {
        self.background(state)
    }

    /// Compatibility shim — `.bind_border_color(signal)` → `.border_color(signal)`.
    pub fn bind_border_color(self, state: impl Into<ColorProp>) -> Self {
        self.border_color(state)
    }

    /// Compatibility shim — `.bind_border_width(signal)` → `.border_width(signal)`.
    pub fn bind_border_width(self, state: impl Into<Prop<f32>>) -> Self {
        self.border_width(state)
    }

    /// Compatibility shim — `.bind_corner_radius(signal)` → `.corner_radius(signal)`.
    pub fn bind_corner_radius(self, state: impl Into<Prop<CornerRadius>>) -> Self {
        self.corner_radius(state)
    }
}

impl Default for RectWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for RectWidget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RectWidget").finish()
    }
}

impl Widget for RectWidget {
    fn build(
        &mut self,
        ctx: &mut bastyde_core::build_context::BuildContext,
    ) -> Vec<bastyde_core::widget_id::WidgetId> {
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.background.register_if_bound(
            self_id,
            registry,
            bastyde_core::binding::BindingLevel::RepaintOnly,
        );
        self.border_color.register_if_bound(
            self_id,
            registry,
            bastyde_core::binding::BindingLevel::RepaintOnly,
        );
        self.border_width.register_if_bound(
            self_id,
            registry,
            bastyde_core::binding::BindingLevel::RepaintOnly,
        );
        self.corner_radius.register_if_bound(
            self_id,
            registry,
            bastyde_core::binding::BindingLevel::RepaintOnly,
        );
        Vec::new()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        // RectWidget has no intrinsic content — it accepts whatever space is offered.
        // With an exact proposal it fills the space; with unspecified it reports 0x0.
        proposal.resolve(0.0, 0.0).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let bg = self.background.resolve(ctx.theme, ctx.effective_enabled);
        let radius = self.corner_radius.get();
        if bg.a() > 0.0 {
            canvas.fill_rounded_rect(bounds, radius, bg);
        }
        let bw = self.border_width.get();
        let bc = self.border_color.resolve(ctx.theme, ctx.effective_enabled);
        if bw > 0.0 && bc.a() > 0.0 {
            canvas.stroke_rounded_rect(bounds, radius, bc, bw);
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::signal::Signal;
    use bastyde_core::widget_tree::WidgetTree;

    #[test]
    fn static_background_paints_correctly() {
        let mut tree = WidgetTree::new();
        tree.add(
            RectWidget::new()
                .background(Color::RED)
                .corner_radius(CornerRadius::uniform(4.0)),
        );
        tree.layout(SizeProposal::exact(100.0, 40.0));
        let frame = tree.render();
        assert_eq!(frame.shapes.len(), 1);
        assert_eq!(frame.shapes[0].color, Color::RED.to_array());
    }

    #[test]
    fn bind_background_reads_from_state() {
        let color = Signal::new(Color::BLUE);
        let mut tree = WidgetTree::new();
        let w = tree.add(
            RectWidget::new()
                .bind_background(color.clone())
                .corner_radius(CornerRadius::uniform(4.0)),
        );
        color.bind_to(
            w,
            tree.binding_registry(),
            bastyde_core::binding::BindingLevel::RepaintOnly,
        );
        tree.layout(SizeProposal::exact(100.0, 40.0));
        let frame = tree.render();
        assert_eq!(frame.shapes[0].color, Color::BLUE.to_array());
    }

    #[test]
    fn bind_background_updates_on_state_change() {
        let color = Signal::new(Color::RED);
        let mut tree = WidgetTree::new();
        let w = tree.add(
            RectWidget::new()
                .bind_background(color.clone())
                .corner_radius(CornerRadius::uniform(4.0)),
        );
        color.bind_to(
            w,
            tree.binding_registry(),
            bastyde_core::binding::BindingLevel::RepaintOnly,
        );

        tree.layout(SizeProposal::exact(100.0, 40.0));
        let frame = tree.render();
        assert_eq!(frame.shapes[0].color, Color::RED.to_array());

        // Change the state
        color.set(Color::GREEN);
        tree.layout(SizeProposal::exact(100.0, 40.0));
        let frame = tree.render();
        assert_eq!(frame.shapes[0].color, Color::GREEN.to_array());
    }
}
