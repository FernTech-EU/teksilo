// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Card — a surface container with optional header, content, and footer slots.
//!
//! `Card` renders an opaque or tinted rounded-rectangle backdrop, an optional
//! drop shadow, and up to three stacked content slots (header / content /
//! footer). It is the standard building block for list-item cards, dashboard
//! tiles, onboarding panels, and any widget that needs a visually distinct
//! raised or outlined surface. Chrome (shadow, background, corner radius,
//! padding) is delegated to the active [`CardStyle`](bastyde_core::styles::CardStyle)
//! so the visual language can be changed per-call (`.style(...)`) or
//! theme-wide via `theme.style_slots.card`.
//!
//! ## When to use
//!
//! - `CardVariant::Elevated` — a dashboard tile or list card that should
//!   "float" above the page surface.
//! - `CardVariant::Outlined` — a bordered grouping box without shadow.
//! - `CardVariant::Plain` — the content sits on the default surface; no
//!   visible chrome (useful for spacing only).
//!
//! ## Accessibility
//!
//! Announces as `Role::Group`. The slots' own accessibility nodes are
//! included in the subtree; the card itself carries no additional AT name.
//!
//! ```rust
//! # use bastyde_widgets::Card;
//! # use bastyde_core::styles::CardVariant;
//! # use bastyde_widgets::primitives::TextWidget;
//! # use bastyde_i18n::lit;
//! let _card = Card::new()
//!     .variant(CardVariant::Elevated)
//!     .content(TextWidget::new(lit!("Hello, card!")));
//! ```

use std::rc::Rc;

use bastyde_canvas::{Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::signal::Prop;
use bastyde_core::styles::{CardStyleConfig, CardVariant, SharedCardStyle};
use bastyde_core::widget::{LayoutContext, PendingChild, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::Shadow;

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
    /// Construct an empty card with no slots and the default `CardVariant::Plain`.
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

    /// Set the header slot (topmost section) to an inline widget.
    pub fn header(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_header = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// Set the header slot to a pre-registered `WidgetId`.
    pub fn header_id(mut self, id: WidgetId) -> Self {
        self.pending_header = Some(PendingChild::Id(id));
        self
    }

    /// Set the main content slot (middle section) to an inline widget.
    pub fn content(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_content = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// Set the main content slot to a pre-registered `WidgetId`.
    pub fn content_id(mut self, id: WidgetId) -> Self {
        self.pending_content = Some(PendingChild::Id(id));
        self
    }

    /// Set the footer slot (bottommost section) to an inline widget.
    pub fn footer(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_footer = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// Set the footer slot to a pre-registered `WidgetId`.
    pub fn footer_id(mut self, id: WidgetId) -> Self {
        self.pending_footer = Some(PendingChild::Id(id));
        self
    }

    /// Override the drop shadow. Accepts a `Shadow` token (see
    /// `bastyde_tokens::Shadow`). The default shadow comes from the active
    /// `CardStyle` for the chosen `CardVariant`.
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
    pub fn style(mut self, style: impl bastyde_core::styles::CardStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    fn resolve_padding(&self, _theme: &bastyde_core::Theme) -> f32 {
        self.padding
            .as_ref()
            .map(|p| p.get())
            .unwrap_or(crate::styles::recipe_card_style::CARD_PADDING)
    }
}

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Card {
    fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
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
            .or_else(|| ctx.theme().style_slots.card.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeCardStyle));
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
    ) -> bastyde_core::widget::LayoutResponse {
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
        builder.set_role(bastyde_core::accesskit::Role::Group);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::widget_tree::WidgetTree;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> bastyde_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    #[test]
    fn card_renders_shadow_and_background() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
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
}
