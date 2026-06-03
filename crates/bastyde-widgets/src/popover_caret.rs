//! `DisclosureCaret` — a 6×6 dp right triangle painted in the
//! bottom-right corner of a popover trigger to indicate "this opens a
//! menu, not a single-action button". Shared between
//! [`PopoverIconButton`](crate::popover_widget::PopoverIconButton)
//! and [`PopoverButton`](crate::popover_widget::PopoverButton).
//!
//! The caret takes a caller-derived `Signal<TextRole>`; each wrapper
//! computes the role from its own interaction state + variant policy
//! (IconButton's embedded vs stand-alone palette, Button's Default /
//! Regular / Flat) and hands the derived signal in. The caret has
//! zero domain knowledge of "which button type" — it just resolves
//! the role against the current theme on every paint.
//!
//! Pointer-pass-through: the caret never absorbs clicks meant for the
//! IconButton/Button sibling beneath it in the parent ZStack. AT-
//! hidden — the popover affordance is announced by the trigger via
//! `set_has_popup` + `set_expanded`.

use bastyde_canvas::{Canvas, Path, Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::TextRole;

/// Width / height of the painted disclosure triangle, in logical pixels.
pub(crate) const CARET_DIM: f32 = 6.0;

/// Inset between the triangle's bottom-right vertex and the host
/// widget's bottom-right corner. Keeps the vertex inside the rounded
/// background (Button uses a 4 dp corner radius, IconButton 8 dp; 2
/// dp clears both).
pub(crate) const CARET_INSET: f32 = 2.0;

/// Layered on top of a popover trigger to paint the disclosure
/// triangle. Layout-transparent (inherits the trigger's bounds via
/// `layout_response`), pointer-pass-through (set in `build`).
#[derive(Debug)]
pub(crate) struct DisclosureCaret {
    /// Caller-derived role signal. The caret repaints when it
    /// changes — typically a `Signal::map(...)` over the trigger's
    /// shared `Signal<InteractionState>`, applying whichever
    /// variant-specific role policy the trigger uses.
    pub(crate) role: Signal<TextRole>,
}

impl Widget for DisclosureCaret {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Decorative chrome: must not intercept pointer events, or it
        // would absorb clicks meant for the trigger sibling sitting
        // beneath it in the parent ZStack. Same trick the inspector's
        // HighlightLayer / HoverProbe use.
        ctx.apply_self_handlers(HandlerSet::new().event_pass_through(true));
        // Repaint when the role flips so the caret tracks the
        // trigger's hover / press / focus / disabled tints.
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.role
            .bind_to(self_id, registry, BindingLevel::RepaintOnly);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        // Inherit the parent's proposal so the caret's bounds match
        // the trigger sibling. The triangle is positioned manually in
        // `paint` at the bottom-right corner of those bounds.
        let w = proposal.width.unwrap_or(0.0);
        let h = proposal.height.unwrap_or(0.0);
        Size::new(w, h).into()
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
        if bounds.width <= CARET_DIM + CARET_INSET || bounds.height <= CARET_DIM + CARET_INSET {
            return;
        }
        let color = self.role.get().resolve(&ctx.theme.colors);
        let right = bounds.x + bounds.width - CARET_INSET;
        let bottom = bounds.y + bounds.height - CARET_INSET;
        // Right-angle vertex at (right, bottom); legs go up by
        // CARET_DIM and left by CARET_DIM. Hypotenuse runs from
        // (right - CARET_DIM, bottom) to (right, bottom - CARET_DIM).
        let mut path = Path::new();
        path.move_to(Point::new(right, bottom));
        path.line_to(Point::new(right - CARET_DIM, bottom));
        path.line_to(Point::new(right, bottom - CARET_DIM));
        path.close();
        canvas.fill_path(&path, color);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Decorative — the popover affordance is announced by the
        // trigger sibling via `set_has_popup` + `set_expanded`.
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
        builder.set_hidden();
    }
}
