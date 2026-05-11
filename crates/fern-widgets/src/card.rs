//! Card — a Panel with shadow and optional header/content/footer slots.
//!
//! Card composes its chrome (shadow + background + corner radius +
//! padding) via the `CardStyle` trait protocol. The default
//! `RecipeCardStyle` honours all four `CardVariant` values (Plain,
//! Elevated, Outlined, Filled) plus per-call manual overrides
//! (background, corner_radius, padding, shadow). Apps that want a
//! different chrome (frosted-glass card, brutalist box, neumorphic
//! raised surface) plug their own `impl CardStyle` per-call
//! (`.style(...)`) or theme-wide (step 8's
//! `ComponentStyles.card = Rc::new(MyCard)`).

use std::rc::Rc;

use fern_canvas::{Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::color_prop::ColorProp;
use fern_core::signal::Prop;
use fern_core::styles::{CardStyleConfig, CardVariant, SharedCardStyle};
use fern_core::widget::{LayoutContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::Shadow;

use crate::primitives::VStack;

/// A card container with shadow, background, and optional header/content/footer.
pub struct Card {
    header_id: Option<WidgetId>,
    content_id: Option<WidgetId>,
    footer_id: Option<WidgetId>,
    pending_header: Option<PendingChild>,
    pending_content: Option<PendingChild>,
    pending_footer: Option<PendingChild>,
    shadow: Option<Shadow>,
    background: Option<ColorProp>,
    corner_radius: Option<Prop<f32>>,
    padding: Option<Prop<f32>>,
    variant: CardVariant,
    style_override: Option<SharedCardStyle>,
    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for Card {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Card")
            .field("variant", &self.variant)
            .finish()
    }
}

impl Card {
    pub fn new() -> Self {
        Self {
            header_id: None,
            content_id: None,
            footer_id: None,
            pending_header: None,
            pending_content: None,
            pending_footer: None,
            shadow: None,
            background: None,
            corner_radius: None,
            padding: None,
            variant: CardVariant::default(),
            style_override: None,
            root_child_id: None,
        }
    }

    pub fn header(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_header = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    pub fn header_id(mut self, id: WidgetId) -> Self {
        self.pending_header = Some(PendingChild::Id(id));
        self
    }

    pub fn content(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_content = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    pub fn content_id(mut self, id: WidgetId) -> Self {
        self.pending_content = Some(PendingChild::Id(id));
        self
    }

    pub fn footer(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_footer = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    pub fn footer_id(mut self, id: WidgetId) -> Self {
        self.pending_footer = Some(PendingChild::Id(id));
        self
    }

    pub fn shadow(mut self, shadow: Shadow) -> Self {
        self.shadow = Some(shadow);
        self
    }

    /// Override the background. Default (unset) is the variant's default
    /// (`SurfaceRole::Main` for Plain/Outlined/Elevated, `SurfaceRole::Raised`
    /// for Filled). Accepts `Color`, a role, or `Signal<Color>`.
    pub fn background(mut self, color: impl Into<ColorProp>) -> Self {
        self.background = Some(color.into());
        self
    }

    /// Override the corner radius (default: theme `components.card.corner_radius`).
    /// Accepts a static `f32` or a reactive `Signal<f32>`.
    pub fn corner_radius(mut self, radius: impl Into<Prop<f32>>) -> Self {
        self.corner_radius = Some(radius.into());
        self
    }

    /// Override the padding (default: theme `components.card.padding`).
    /// Accepts a static `f32` or a reactive `Signal<f32>`.
    pub fn padding(mut self, padding: impl Into<Prop<f32>>) -> Self {
        self.padding = Some(padding.into());
        self
    }

    /// Pick the design-language variant. Default `Plain`. The active
    /// `CardStyle` decides what each variant means visually (the IntUI
    /// default maps Plain → no shadow + surface_main, Elevated →
    /// shadow_md + surface_main, Outlined → border + surface_main,
    /// Filled → shadow_md + surface_raised).
    pub fn variant(mut self, variant: CardVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Per-call style override. Replaces the theme-wide default
    /// `CardStyle` for just this Card instance.
    pub fn style(mut self, style: impl fern_core::styles::CardStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    fn resolve_padding(&self, theme: &fern_core::Theme) -> f32 {
        self.padding
            .as_ref()
            .map(|p| p.get())
            .unwrap_or(theme.components.card.padding)
    }
}

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Card {
    fn build(&mut self, ctx: &mut fern_core::build_context::BuildContext) -> Vec<WidgetId> {
        if let Some(h) = self.pending_header.take() {
            self.header_id = Some(match h {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        if let Some(c) = self.pending_content.take() {
            self.content_id = Some(match c {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        if let Some(f) = self.pending_footer.take() {
            self.footer_id = Some(match f {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }

        // Compose the three slots into a single content widget — a VStack
        // with `padding/2` spacing between sections (mirrors the
        // pre-refactor in-card section spacing). Empty if all three are
        // None (the style still gets a `content: WidgetId` to wrap).
        let pad = self.resolve_padding(ctx.theme());
        let spacing = pad * 0.5;
        let mut stack = VStack::new().spacing(spacing);
        for slot in [self.header_id, self.content_id, self.footer_id]
            .into_iter()
            .flatten()
        {
            stack = stack.add_child(slot);
        }
        let content = ctx.add(stack);

        let style: SharedCardStyle = self
            .style_override
            .clone()
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeCardStyle::default()));
        let cfg = CardStyleConfig {
            content,
            is_hovered: None,
            variant: self.variant,
            background_override: self.background.clone(),
            corner_radius_override: self.corner_radius.clone(),
            padding_override: self.padding.clone(),
            shadow_override: self.shadow,
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
            child.origin = Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
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
    fn card_renders_shadow_and_background() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let _content = tree.add(FixedLeaf(100.0, 50.0));
        tree.add(
            Card::new()
                .variant(CardVariant::Elevated)
                .content(FixedLeaf(100.0, 50.0)),
        );
        tree.layout(SizeProposal::exact(200.0, 200.0));
        let frame = tree.render();
        // Should have shapes for shadow + background
        assert!(
            !frame.shapes.is_empty() || !frame.shadows.is_empty(),
            "card should render shadow and/or background"
        );
    }

    #[test]
    fn card_with_header_and_content() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.add(
            Card::new()
                .header(FixedLeaf(100.0, 30.0))
                .content(FixedLeaf(100.0, 50.0)),
        );
        tree.layout(SizeProposal::exact(200.0, 200.0));
        // Should not crash and should layout
    }
}
