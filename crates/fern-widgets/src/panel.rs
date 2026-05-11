//! Panel — a themed container with background, border, corner radius, and padding.
//!
//! Like Qt's QFrame: a single-child wrapper that provides visual framing.
//! Visual properties come from the theme by default but can be overridden.
//!
//! Panel composes its chrome via the `PanelStyle` trait protocol.
//! The default `RecipePanelStyle` honours all four `PanelVariant` values
//! (Plain / Sunken / Raised / Highlighted) plus per-call manual overrides
//! (background, border_color, border_width, corner_radius, padding).
//! Apps that want a different chrome (frosted-glass panel, brutalist
//! frame) plug their own `impl PanelStyle` per-call (`.style(...)`) or
//! theme-wide (step 8's `ComponentStyles.panel = Rc::new(MyPanel)`).

use std::rc::Rc;

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::color_prop::ColorProp;
use fern_core::signal::Prop;
use fern_core::styles::{PanelStyleConfig, PanelVariant, SharedPanelStyle};
use fern_core::widget::{LayoutContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
#[cfg(test)]
use fern_tokens::Color;

/// A themed container with background, border, corner radius, and padding.
pub struct Panel {
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
    background: Option<ColorProp>,
    border_color: Option<ColorProp>,
    border_width: Option<Prop<f32>>,
    corner_radius: Option<Prop<f32>>,
    padding: Option<Prop<f32>>,
    variant: PanelVariant,
    style_override: Option<SharedPanelStyle>,
    root_child_id: Option<WidgetId>,
    a11y_presentational: bool,
}

impl std::fmt::Debug for Panel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Panel")
            .field("variant", &self.variant)
            .field("a11y_presentational", &self.a11y_presentational)
            .finish()
    }
}

impl Panel {
    pub fn new() -> Self {
        Self {
            child_id: None,
            pending_child: None,
            background: None,
            border_color: None,
            border_width: None,
            corner_radius: None,
            padding: None,
            variant: PanelVariant::default(),
            style_override: None,
            root_child_id: None,
            a11y_presentational: false,
        }
    }

    /// Pick the design-language variant. Default `Plain`. The active
    /// `PanelStyle` decides what each variant means visually (the
    /// IntUI default maps Plain → `surface_main`, Sunken →
    /// `surface_sunken`, Raised → `surface_raised`, Highlighted →
    /// `accent_subtle_bg`, with matching border defaults).
    pub fn variant(mut self, variant: PanelVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Per-call style override. Replaces the theme-wide default
    /// `PanelStyle` for just this Panel instance — same role as
    /// `Button::style(...)`. Manual overrides (`background`,
    /// `border_color`, etc.) are still passed to the style via
    /// `PanelStyleConfig`; custom styles are free to honour or ignore
    /// them.
    pub fn style(mut self, style: impl fern_core::styles::PanelStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Mark the panel as presentational for assistive tech: the panel's
    /// own a11y node is hidden so its wrapping chrome (background,
    /// border, padding) doesn't introduce a spurious `Group` node
    /// between an outer widget (Toolbar, StatusBar, etc.) and the
    /// real content. Children remain visible in the a11y tree.
    pub fn a11y_presentational(mut self) -> Self {
        self.a11y_presentational = true;
        self
    }

    /// Set child by pre-registered ID.
    pub fn child_id(mut self, id: WidgetId) -> Self {
        self.pending_child = Some(PendingChild::Id(id));
        self
    }

    /// Set an inline child widget (deferred insertion).
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_child = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// Override the background. Accepts `Color`, a [`SurfaceRole`](fern_tokens::SurfaceRole),
    /// or a `Signal<Color>`. Default (unset) is `SurfaceRole::Main`.
    pub fn background(mut self, color: impl Into<ColorProp>) -> Self {
        self.background = Some(color.into());
        self
    }

    /// Override the border color. Accepts `Color`, a [`BorderRole`](fern_tokens::BorderRole),
    /// or a `Signal<Color>`. Default (unset) is `BorderRole::Default`.
    pub fn border_color(mut self, color: impl Into<ColorProp>) -> Self {
        self.border_color = Some(color.into());
        self
    }

    /// Override the border width (default: 0 — no border).
    /// Accepts a static `f32` or a reactive `Signal<f32>`.
    pub fn border_width(mut self, width: impl Into<Prop<f32>>) -> Self {
        self.border_width = Some(width.into());
        self
    }

    /// Override the corner radius (default: theme `radius_popup`).
    /// Accepts a static `f32` or a reactive `Signal<f32>`.
    pub fn corner_radius(mut self, radius: impl Into<Prop<f32>>) -> Self {
        self.corner_radius = Some(radius.into());
        self
    }

    /// Override the padding (default: theme `components.panel.padding`).
    /// Accepts a static `f32` or a reactive `Signal<f32>`.
    pub fn padding(mut self, padding: impl Into<Prop<f32>>) -> Self {
        self.padding = Some(padding.into());
        self
    }

}

impl Default for Panel {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Panel {
    fn build(&mut self, ctx: &mut fern_core::build_context::BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        let content = match self.child_id {
            Some(id) => id,
            // Headless / empty panel — emit a zero-size placeholder so
            // the style still has a `content: WidgetId` to wrap.
            None => ctx.add(crate::primitives::FixedSize::new().bind_width(0.0).bind_height(0.0)),
        };

        let style: SharedPanelStyle = self
            .style_override
            .clone()
            .unwrap_or_else(|| Rc::new(crate::styles::RecipePanelStyle::default()));
        let cfg = PanelStyleConfig {
            content,
            variant: self.variant,
            background_override: self.background.clone(),
            border_color_override: self.border_color.clone(),
            border_width_override: self.border_width.clone(),
            corner_radius_override: self.corner_radius.clone(),
            padding_override: self.padding.clone(),
        };
        let root_id = style.make_body(&cfg, ctx);
        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        if let Some(root) = self.root_child_id
            && let Some(size) = ctx.child_size(root, proposal)
        {
            return (size).into();
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
            child.origin = fern_canvas::Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        if self.a11y_presentational {
            builder.set_hidden();
            return;
        }
        builder.set_role(fern_core::accesskit::Role::Group);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;
    use fern_core::Theme;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> fern_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    #[test]
    fn panel_adds_padding_to_child_size() {
        let theme = fern_core::presets::intui::light();
        let mut tree = WidgetTree::new().with_theme(theme.clone());
        let child = tree.add(FixedLeaf(80.0, 40.0));
        let panel = tree.add(Panel::new().padding(10.0).child_id(child));
        tree.layout(SizeProposal::unspecified());

        let pb = tree.bounds(panel);
        assert!((pb.width - 100.0).abs() < 0.01); // 80 + 10*2
        assert!((pb.height - 60.0).abs() < 0.01); // 40 + 10*2
    }

    #[test]
    fn panel_child_positioned_with_padding() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let child = tree.add(FixedLeaf(80.0, 40.0));
        let _panel = tree.add(Panel::new().padding(12.0).child_id(child));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let cb = tree.bounds(child);
        assert!((cb.x - 12.0).abs() < 0.01);
        assert!((cb.y - 12.0).abs() < 0.01);
    }

    #[test]
    fn panel_paints_background() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let child = tree.add(FixedLeaf(50.0, 30.0));
        let _panel = tree.add(
            Panel::new()
                .background(Color::RED)
                .corner_radius(8.0)
                .child_id(child),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let frame = tree.render();
        assert!(
            !frame.shapes.is_empty(),
            "panel should render a background shape"
        );
    }
}
