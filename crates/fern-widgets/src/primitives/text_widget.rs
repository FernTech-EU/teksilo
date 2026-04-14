use std::cell::RefCell;
use std::rc::Rc;

use fern_canvas::{Canvas, EllipsisMode, Rect, Size, SizeProposal, TextOverflow};
use fern_tokens::{Color, TextStyle};

use fern_core::accessibility::AccessNodeBuilder;
use fern_core::signal::Prop;
use fern_core::widget::{LayoutContext, PaintContext, Widget};
use fern_i18n::LocalizedString;

/// A leaf widget that renders text via the TextBackend.
///
/// Defaults to [`TextOverflow::Wrap`]: long text wraps onto multiple
/// lines and the widget grows vertically. Single-line widgets (button
/// labels, menu items, tab headers, etc.) opt out with
/// `.overflow(TextOverflow::Ellipsis(EllipsisMode::Trailing))`.
///
/// Text and color can be static or bound to reactive state.
pub struct TextWidget {
    text: Prop<String>,
    color: Prop<Color>,
    style: TextStyle,
    overflow: TextOverflow,
    max_lines: Option<usize>,
    text_backend: Option<Rc<RefCell<dyn fern_canvas::TextBackend>>>,
}

impl std::fmt::Debug for TextWidget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextWidget").finish()
    }
}

impl TextWidget {
    /// Construct a text widget whose content is a `LocalizedString`. The
    /// text may come from `tr!(...)` (reactive, re-resolves on locale
    /// change) or from `LocalizedString::literal("…")` for genuinely
    /// non-translated strings.
    pub fn new(text: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = text.into();
        Self {
            text: Prop::from(ls),
            color: Prop::Static(Color::BLACK),
            style: TextStyle::default(),
            overflow: TextOverflow::default(),
            max_lines: None,
            text_backend: None,
        }
    }

    /// Shim (permanent, `#[doc(hidden)]`) — wraps a raw string in
    /// `LocalizedString::literal` for tests and scaffolding where
    /// translation is overkill. Production code uses
    /// `new(tr!(...))`; the `*_literal` suffix is the grep marker for
    /// untranslated strings alongside `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn new_literal(text: impl Into<String>) -> Self {
        Self::new(LocalizedString::literal(text))
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = Prop::Static(color);
        self
    }

    pub fn style(mut self, style: TextStyle) -> Self {
        self.style = style;
        self
    }

    /// Set how the widget handles text that doesn't fit in the proposed
    /// width. Default is [`TextOverflow::Wrap`].
    pub fn overflow(mut self, overflow: TextOverflow) -> Self {
        self.overflow = overflow;
        self
    }

    /// Shorthand for `.overflow(TextOverflow::Ellipsis(EllipsisMode::Trailing))`.
    /// Use this on labels inside single-line containers (buttons, menu items,
    /// tab headers, badges, status bar cells, etc.) so long text truncates
    /// with a trailing "…" instead of wrapping onto multiple lines.
    pub fn single_line(self) -> Self {
        self.overflow(TextOverflow::Ellipsis(EllipsisMode::Trailing))
    }

    /// Cap the paragraph at `n` lines when wrapping. Only meaningful
    /// in [`TextOverflow::Wrap`] mode — ignored for ellipsis modes.
    /// Lines beyond the cap are silently dropped.
    pub fn max_lines(mut self, n: usize) -> Self {
        self.max_lines = Some(n);
        self
    }

    pub fn text_backend(mut self, backend: Rc<RefCell<dyn fern_canvas::TextBackend>>) -> Self {
        self.text_backend = Some(backend);
        self
    }

    /// Bind the text content to a reactive state.
    pub fn bind_text(mut self, state: impl Into<Prop<String>>) -> Self {
        self.text = state.into();
        self
    }

    /// Bind the text color to a reactive state.
    pub fn bind_color(mut self, state: impl Into<Prop<Color>>) -> Self {
        self.color = state.into();
        self
    }

    /// Get the current text value (resolves from state if bound).
    pub fn text(&self) -> String {
        self.text.get()
    }
}

impl Widget for TextWidget {
    fn build(
        &mut self,
        ctx: &mut fern_core::build_context::BuildContext,
    ) -> Vec<fern_core::widget_id::WidgetId> {
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.text.register_if_bound(
            self_id,
            registry,
            fern_core::binding::BindingLevel::Relayout,
        );
        self.color.register_if_bound(
            self_id,
            registry,
            fern_core::binding::BindingLevel::RepaintOnly,
        );
        Vec::new()
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        let text = self.text.get();
        let Some(backend) = self.text_backend.as_ref().or(ctx.text_backend) else {
            // Mock fallback when no backend is available (e.g. very early
            // bootstrap). Assume 8px/char for measurement.
            let width = text.len() as f32 * 8.0;
            let height = 16.0;
            let w = match proposal.width {
                Some(max) => width.min(max),
                None => width,
            };
            return Size::new(w, height);
        };
        let mut backend = backend.borrow_mut();

        match self.overflow {
            TextOverflow::Wrap => match proposal.width {
                Some(w) => {
                    let layout = backend.layout_paragraph(&text, &self.style, w, self.max_lines);
                    Size::new(layout.width, layout.height)
                }
                None => {
                    // Unconstrained width: no basis for wrapping, so measure
                    // as a single line.
                    let layout = backend.layout_single_line(&text, &self.style, None);
                    Size::new(layout.width, layout.height)
                }
            },
            TextOverflow::Ellipsis(EllipsisMode::Trailing) => {
                // text-typeset truncates with trailing "…" when a max_width
                // is supplied — let it do the work.
                let layout = backend.layout_single_line(&text, &self.style, proposal.width);
                Size::new(layout.width, layout.height)
            }
            TextOverflow::Ellipsis(mode) => {
                // Middle / Leading: compute the truncated display string
                // first, then measure it unconstrained.
                let Some(max_w) = proposal.width else {
                    let layout = backend.layout_single_line(&text, &self.style, None);
                    return Size::new(layout.width, layout.height);
                };
                let truncated = fern_canvas::ellipsis::ellipsize(
                    &text,
                    &self.style,
                    max_w,
                    mode,
                    &mut *backend,
                );
                let layout = backend.layout_single_line(&truncated, &self.style, None);
                Size::new(layout.width, layout.height)
            }
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        let text = self.text.get();
        let color = self.color.get();
        match self.overflow {
            TextOverflow::Wrap => {
                canvas.draw_paragraph(&text, bounds, &self.style, color, self.max_lines);
            }
            TextOverflow::Ellipsis(EllipsisMode::Trailing) => {
                canvas.draw_text(&text, bounds, &self.style, color);
            }
            TextOverflow::Ellipsis(mode) => {
                // Produce the truncated display string via the canvas's
                // backend and hand it to draw_text.
                let truncated = match canvas.text_backend() {
                    Some(backend) => fern_canvas::ellipsis::ellipsize(
                        &text,
                        &self.style,
                        bounds.width,
                        mode,
                        &mut *backend.borrow_mut(),
                    ),
                    None => text.clone(),
                };
                canvas.draw_text(&truncated, bounds, &self.style, color);
            }
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        let text = self.text.get();
        builder.set_role(fern_core::accesskit::Role::Label);
        builder.set_name(&text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_canvas::MockTextBackend;
    use fern_core::signal::Signal;
    use fern_core::widget_tree::WidgetTree;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn tree_with_mock_backend() -> WidgetTree {
        WidgetTree::new().with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())))
    }

    #[test]
    fn bind_text_renders_state_value() {
        let text = Signal::new("Hello".to_string());
        let mut tree = WidgetTree::new();
        let w = tree.add(TextWidget::new_literal("").bind_text(text.clone()));
        text.bind_to(
            w,
            tree.binding_registry(),
            fern_core::binding::BindingLevel::Relayout,
        );
        tree.layout(SizeProposal::exact(200.0, 40.0));

        assert_eq!(tree.text_content(w), Some("Hello".to_string()));
    }

    #[test]
    fn bind_text_updates_on_state_change() {
        let text = Signal::new("Hello".to_string());
        let mut tree = WidgetTree::new();
        let w = tree.add(TextWidget::new_literal("").bind_text(text.clone()));
        text.bind_to(
            w,
            tree.binding_registry(),
            fern_core::binding::BindingLevel::Relayout,
        );

        tree.layout(SizeProposal::exact(200.0, 40.0));
        assert_eq!(tree.text_content(w), Some("Hello".to_string()));

        text.set("World".to_string());
        tree.layout(SizeProposal::exact(200.0, 40.0));
        assert_eq!(tree.text_content(w), Some("World".to_string()));
    }

    // -------------------------------------------------------------------
    // Overflow modes
    // -------------------------------------------------------------------

    #[test]
    fn wrap_is_the_default_mode() {
        let w = TextWidget::new_literal("Hello");
        assert_eq!(w.overflow, TextOverflow::Wrap);
    }

    #[test]
    fn wrap_grows_vertically_in_narrow_proposal() {
        // MockTextBackend: 8px/char, 16px line height. "one two three four"
        // = 18 bytes × 8 = 144px wide single-line. At max_width 50 it
        // should wrap across several lines.
        let mut tree = tree_with_mock_backend();
        let w = tree.add(TextWidget::new_literal("one two three four"));
        tree.layout(SizeProposal {
            width: Some(50.0),
            height: None,
        });
        let b = tree.bounds(w);
        assert!(
            b.height > 16.0,
            "wrapped text should be taller than one line (got {})",
            b.height
        );
        assert!(
            b.width <= 50.0 + 1.0,
            "wrapped text should stay within proposal width (got {})",
            b.width
        );
    }

    #[test]
    fn wrap_falls_back_to_single_line_when_proposal_width_is_none() {
        let mut tree = tree_with_mock_backend();
        let w = tree.add(TextWidget::new_literal("one two three"));
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        let b = tree.bounds(w);
        assert!(
            (b.height - 16.0).abs() < 0.1,
            "unbounded width should produce a single line (got {})",
            b.height
        );
    }

    #[test]
    fn wrap_max_lines_caps_paragraph_height() {
        // "one two three four five" wraps to multiple lines; cap at 2.
        let mut tree = tree_with_mock_backend();
        let w = tree.add(TextWidget::new_literal("one two three four five six seven").max_lines(2));
        tree.layout(SizeProposal {
            width: Some(40.0),
            height: None,
        });
        let b = tree.bounds(w);
        assert!(
            b.height <= 32.0 + 0.1,
            "max_lines(2) should cap height at 2 × 16px (got {})",
            b.height
        );
    }

    #[test]
    fn trailing_ellipsis_clamps_width_to_proposal() {
        // MockTextBackend clamps at max_width for single-line measurement.
        let mut tree = tree_with_mock_backend();
        let w = tree.add(
            TextWidget::new_literal("a very long piece of text")
                .overflow(TextOverflow::Ellipsis(EllipsisMode::Trailing)),
        );
        tree.layout(SizeProposal {
            width: Some(40.0),
            height: None,
        });
        let b = tree.bounds(w);
        assert!(b.width <= 40.0 + 0.1);
        assert!(
            (b.height - 16.0).abs() < 0.1,
            "ellipsized text should stay on one line"
        );
    }

    #[test]
    fn middle_ellipsis_produces_narrow_single_line_layout() {
        let mut tree = tree_with_mock_backend();
        let w = tree.add(
            TextWidget::new_literal("abcdefghijklmnop")
                .overflow(TextOverflow::Ellipsis(EllipsisMode::Middle)),
        );
        tree.layout(SizeProposal {
            width: Some(80.0),
            height: None,
        });
        let b = tree.bounds(w);
        assert!(
            b.width <= 80.0 + 0.1,
            "middle-ellipsized width should fit the proposal (got {})",
            b.width
        );
        assert!(
            (b.height - 16.0).abs() < 0.1,
            "ellipsized text should stay on one line"
        );
    }

    #[test]
    fn leading_ellipsis_produces_narrow_single_line_layout() {
        let mut tree = tree_with_mock_backend();
        let w = tree.add(
            TextWidget::new_literal("abcdefghijklmnop")
                .overflow(TextOverflow::Ellipsis(EllipsisMode::Leading)),
        );
        tree.layout(SizeProposal {
            width: Some(80.0),
            height: None,
        });
        let b = tree.bounds(w);
        assert!(
            b.width <= 80.0 + 0.1,
            "leading-ellipsized width should fit the proposal (got {})",
            b.width
        );
    }

    #[test]
    fn single_line_shorthand_matches_trailing_ellipsis() {
        let a = TextWidget::new_literal("hi").single_line();
        let b =
            TextWidget::new_literal("hi").overflow(TextOverflow::Ellipsis(EllipsisMode::Trailing));
        assert_eq!(a.overflow, b.overflow);
    }
}
