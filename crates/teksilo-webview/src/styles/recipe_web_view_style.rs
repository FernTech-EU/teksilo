// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default [`WebViewStyle`] implementation reading IntUI tokens, plus the
//! tiny core-only overlay leaf it builds.
//!
//! `teksilo-webview` deliberately does NOT depend on `teksilo-widgets` (so
//! apps that don't embed web content pay zero compile time for the widget
//! catalog), so the default overlay can't use `Spinner` / `TextWidget` /
//! `ZStack`. It is instead a minimal self-contained container that fills its
//! bounds with a state-derived surface tint behind the app-supplied overlay
//! content: a subtle "loading" wash before the first page paint, an error
//! wash on failure, and fully transparent once the engine surface is showing.
//! Apps that want a richer overlay (animated spinner, retry button) install
//! their own [`WebViewStyle`] via `WebView::style` or
//! `theme.style_slots.web_view`.

use teksilo_canvas::{Canvas, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{WebViewStyle, WebViewStyleConfig, WebViewVisualState};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{BorderRole, CornerRadius};

/// Focus-ring stroke width, in logical pixels. Two, not the usual one: the ring
/// is drawn against page content the toolkit did not choose and cannot know the
/// contrast of, so it is deliberately the heavier of the framework's two.
const FOCUS_RING_WIDTH: f32 = 2.0;

/// Default IntUI web-view style. Stateless; reads theme tokens at paint time.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeWebViewStyle;

impl WebViewStyle for RecipeWebViewStyle {
    fn make_body(&self, cfg: &WebViewStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        ctx.add(WebViewOverlay {
            state: cfg.state.clone(),
            focused: cfg.focused.clone(),
            content: cfg.content,
        })
    }
}

/// State-tinted fill container — the default loading/error wash, with the
/// app-supplied overlay content composited on top.
#[derive(Debug)]
struct WebViewOverlay {
    state: Signal<WebViewVisualState>,
    focused: Signal<bool>,
    content: WidgetId,
}

impl Widget for WebViewOverlay {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Repaint (not relayout) when the lifecycle state flips.
        let self_id = ctx.self_id();
        self.state
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        self.focused
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        // Adopt the pre-built overlay content as our single child.
        vec![self.content]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        // Fill whatever the parent proposes — the engine surface and overlay
        // both occupy the full WebView bounds.
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
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        // Painted before children, so the wash sits behind the overlay content.
        let role = self.state.get().surface_role();
        let color = role.resolve(&ctx.theme.colors);
        if color.a() > 0.0 {
            canvas.fill_rounded_rect(bounds, CornerRadius::ZERO, color);
        }

        // Focus ring. Inset by the full stroke width rather than centred on the
        // edge, because the engine subview is composited *over* this paint —
        // a ring straddling the boundary would have its inner half covered by
        // the page and read as half as thick as it is.
        if self.focused.get() {
            let w = FOCUS_RING_WIDTH;
            let rect = Rect::new(
                bounds.x + w * 0.5,
                bounds.y + w * 0.5,
                (bounds.width - w).max(0.0),
                (bounds.height - w).max(0.0),
            );
            canvas.stroke_rect(rect, BorderRole::Focused.resolve(&ctx.theme.colors), w);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational — the WebView composite node owns the a11y story.
        builder.set_hidden();
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.content]
    }
}
