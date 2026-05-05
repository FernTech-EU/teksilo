//! Per-column filter popover UI.
//!
//! `HeaderCell` renders a small filter glyph (the unconditional
//! [`FilterIndicator`] paint widget) after the sort indicator when the
//! column is `filterable`. With the `rich-text` feature enabled,
//! tapping the glyph opens a [`Popover`] anchored to it whose content
//! is a [`FilterPopoverContent`] widget — a [`TextInput`] with a
//! trailing [`BuiltInButton::clear`] bound to the table's
//! `filters_signal[col_id]` slot. Without `rich-text`, the popover
//! trigger is not built (callers can still drive `filters_signal`
//! programmatically).
//!
//! [`Popover`]: crate::popover::Popover
//! [`TextInput`]: crate::text_input::TextInput
//! [`BuiltInButton::clear`]: crate::built_in_button::BuiltInButton::clear

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::widget::{LayoutContext, PaintContext, Widget};
use fern_tokens::TextRole;

/// Tiny header-cell affordance — a stylized funnel glyph that opens
/// the filter popover when tapped. Tints accent when the column has
/// an active filter, secondary otherwise.
pub(crate) struct FilterIndicator {
    size: f32,
    active: bool,
}

impl FilterIndicator {
    pub(crate) fn new(size: f32, active: bool) -> Self {
        Self { size, active }
    }
}

impl std::fmt::Debug for FilterIndicator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilterIndicator")
            .field("size", &self.size)
            .field("active", &self.active)
            .finish()
    }
}

impl Widget for FilterIndicator {
    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        Size::new(self.size, self.size).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut fern_canvas::Canvas, ctx: &PaintContext) {
        let color = if self.active {
            TextRole::Accent.resolve(&ctx.theme.colors)
        } else {
            TextRole::Secondary.resolve(&ctx.theme.colors)
        };
        // Funnel: top horizontal bar; two converging diagonals; small
        // stem at the bottom. Drawn as filled rectangles so it works
        // without invoking the path pipeline for such a tiny glyph.
        let cx = bounds.x + bounds.width * 0.5;
        let cy = bounds.y + bounds.height * 0.5;
        let r = bounds.width.min(bounds.height) * 0.42;
        // Top bar
        canvas.fill_rect(
            Rect::new(cx - r, cy - r, r * 2.0, (r * 0.30).max(1.0)),
            color,
        );
        // Diagonals approximated with two thin trapezoidal bars.
        let stem_h = (r * 0.45).max(1.0);
        let stem_w = (r * 0.30).max(1.0);
        canvas.fill_rect(
            Rect::new(cx - stem_w * 0.5, cy - r * 0.4, stem_w, r * 1.05),
            color,
        );
        // Stem
        canvas.fill_rect(
            Rect::new(
                cx - (r * 0.14).max(0.5),
                cy + r * 0.4,
                (r * 0.28).max(1.0),
                stem_h,
            ),
            color,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }
}

#[cfg(feature = "rich-text")]
pub(crate) use rich::FilterPopoverContent;

#[cfg(feature = "rich-text")]
mod rich {
    use std::cell::Cell;
    use std::rc::Rc;

    use fern_canvas::{Rect, SizeProposal};
    use fern_core::build_context::BuildContext;
    use fern_core::signal::Signal;
    use fern_core::widget::{LayoutContext, Widget, WidgetPlacement};
    use fern_core::widget_id::WidgetId;

    use crate::built_in_button::BuiltInButton;
    use crate::text_input::TextInput;

    /// Content widget for the per-column filter popover. A
    /// [`TextInput`] bound to a `Signal<String>` with a trailing
    /// [`BuiltInButton::clear`] that empties the field — and via the
    /// `on_change` bridge, the upstream `filters_signal[col_id]` slot.
    pub(crate) struct FilterPopoverContent {
        text: Signal<String>,
        placeholder: String,
        #[allow(clippy::type_complexity)]
        on_change: Option<Rc<dyn Fn(&str)>>,
        /// Slot written to by `build()` with the inner `TextInput`'s
        /// WidgetId so the Popover's open handler can request focus on
        /// it immediately.
        focus_slot: Option<Rc<Cell<Option<WidgetId>>>>,
        root_child_id: Option<WidgetId>,
    }

    impl FilterPopoverContent {
        pub(crate) fn new(initial: impl Into<String>) -> Self {
            Self {
                text: Signal::new(initial.into()),
                placeholder: String::from("Filter…"),
                on_change: None,
                focus_slot: None,
                root_child_id: None,
            }
        }

        #[allow(dead_code)]
        pub(crate) fn placeholder(mut self, text: impl Into<String>) -> Self {
            self.placeholder = text.into();
            self
        }

        pub(crate) fn on_change(mut self, f: impl Fn(&str) + 'static) -> Self {
            self.on_change = Some(Rc::new(f));
            self
        }

        pub(crate) fn focus_slot(mut self, slot: Rc<Cell<Option<WidgetId>>>) -> Self {
            self.focus_slot = Some(slot);
            self
        }
    }

    impl std::fmt::Debug for FilterPopoverContent {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("FilterPopoverContent")
                .field("text_len", &self.text.get().len())
                .field("placeholder", &self.placeholder)
                .finish()
        }
    }

    impl Widget for FilterPopoverContent {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            // Forward any text change into the user-supplied callback.
            // The effect is scoped to this build, so a popover rebuild
            // refreshes the wiring.
            if let Some(cb) = self.on_change.clone() {
                ctx.effect(&self.text, move |s| {
                    cb(s);
                });
            }

            let text = self.text.clone();
            let clear = BuiltInButton::clear().on_activate_fn({
                let text = text.clone();
                move |_| {
                    if !text.get().is_empty() {
                        text.set(String::new());
                    }
                }
            });

            let input_id = ctx.add(
                TextInput::new(text)
                    .placeholder(self.placeholder.clone())
                    .trailing_slot(clear),
            );
            if let Some(slot) = &self.focus_slot {
                slot.set(Some(input_id));
            }

            self.root_child_id = Some(input_id);
            vec![input_id]
        }

        fn layout_response(
            &self,
            proposal: SizeProposal,
            ctx: &LayoutContext,
        ) -> fern_core::widget::LayoutResponse {
            self.root_child_id
                .and_then(|id| ctx.child_size(id, proposal))
                .unwrap_or_else(|| proposal.resolve(280.0, 32.0))
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

        fn children(&self) -> Vec<WidgetId> {
            self.root_child_id.into_iter().collect()
        }
    }
}
