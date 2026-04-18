use std::cell::{Cell, RefCell};
use std::rc::Rc;

use fern_canvas::text_backend::{HitTarget, TextLayout};
use fern_canvas::{Canvas, EllipsisMode, Point, Rect, Size, SizeProposal, TextOverflow};
use fern_tokens::{Color, TextStyle};

use fern_core::accessibility::AccessNodeBuilder;
use fern_core::color_prop::{ColorProp, TextStyleProp};
use fern_core::signal::Prop;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, PaintContext, Widget};
use fern_core::widget_builder::HandlerSet;
use fern_i18n::LocalizedString;
use fern_tokens::TextRole;

/// A leaf widget that renders text via the TextBackend.
///
/// Defaults to [`TextOverflow::Wrap`]: long text wraps onto multiple
/// lines and the widget grows vertically. Single-line widgets (button
/// labels, menu items, tab headers, etc.) opt out with
/// `.overflow(TextOverflow::Ellipsis(EllipsisMode::Trailing))`.
///
/// Text and color can be static or bound to reactive state.
/// Closure type for link click/hover dispatch.
type LinkClickHandler = Rc<dyn Fn(&str, &mut EventContext)>;
type LinkHoverHandler = Rc<dyn Fn(&str, bool, Rect, &mut EventContext)>;

pub struct TextWidget {
    text: Prop<String>,
    /// Foreground color. Defaults to [`TextRole::Primary`], which the paint
    /// pass resolves against the current theme — so `TextWidget::new("...")`
    /// follows theme switches without any explicit binding.
    color: ColorProp,
    /// Text style. Defaults to [`TextStyleRole::Body`](fern_tokens::TextStyleRole);
    /// resolved against the current typography tokens every time the widget
    /// is laid out or painted, so theme switches update font metrics without
    /// a rebuild.
    style: TextStyleProp,
    overflow: TextOverflow,
    max_lines: Option<usize>,
    text_backend: Option<Rc<RefCell<dyn fern_canvas::TextBackend>>>,
    /// When enabled, text is parsed as inline markup
    /// (`[label](url)`, `*italic*`, `**bold**`) and link metadata is
    /// emitted into the layout for hit-testing and per-span coloring.
    markup: bool,
    on_link_click: Option<LinkClickHandler>,
    on_link_hover: Option<LinkHoverHandler>,
    /// Last laid-out markup layout. Shared with the event handler
    /// closures via `Rc<RefCell<..>>` so taps can hit-test against the
    /// most recently measured spans.
    last_layout: Rc<RefCell<Option<TextLayout>>>,
    /// Last paint bounds in window-local coordinates. Updated from
    /// `paint()` (the only hook with access to bounds) and read from
    /// the `on_tap` / `on_pointer_event` closures to convert the
    /// window-local event position into widget-local before
    /// hit-testing the layout spans.
    ///
    /// Necessary because the framework's tap/pointer handlers are
    /// documented as receiving widget-local positions but actually
    /// receive window-local — there is no transformation in
    /// `dispatch_recognized_gesture` or `dispatch_to_widget`.
    last_bounds: Rc<Cell<Rect>>,
    /// Currently-hovered link URL (shared with the pointer-event
    /// closure). Used to detect enter/leave transitions between link
    /// spans inside a single widget.
    hovered_link: Rc<RefCell<Option<String>>>,
    /// When true, this TextWidget emits no accessibility node at all
    /// (no role, no name, no synthetic link children). Controls that
    /// own their accessible name — Button, Checkbox, MenuItem, etc. —
    /// hide their label children so the text doesn't duplicate the
    /// parent's announced name in the a11y tree.
    a11y_hidden: bool,
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
            color: ColorProp::TextRole(TextRole::Primary),
            style: TextStyleProp::default(),
            overflow: TextOverflow::default(),
            max_lines: None,
            text_backend: None,
            markup: false,
            on_link_click: None,
            on_link_hover: None,
            last_layout: Rc::new(RefCell::new(None)),
            last_bounds: Rc::new(Cell::new(Rect::new(0.0, 0.0, 0.0, 0.0))),
            hovered_link: Rc::new(RefCell::new(None)),
            a11y_hidden: false,
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

    /// Set the text color. Accepts any `impl Into<ColorProp>`:
    ///
    /// - A raw `Color` — a frozen literal.
    /// - A [`TextRole`] — resolved against the theme at paint time
    ///   (reactive across theme switches).
    /// - A `Signal<Color>` — reactive state (typically interaction-driven).
    ///
    /// The default role is [`TextRole::Primary`], so `.color(...)` is only
    /// needed when a label wants a non-default theme role (Secondary,
    /// Error, Accent, ...) or a custom color.
    pub fn color(mut self, color: impl Into<ColorProp>) -> Self {
        self.color = color.into();
        self
    }

    /// Set the text style. Accepts a raw [`TextStyle`], a
    /// [`TextStyleRole`](fern_tokens::TextStyleRole), or any value implementing
    /// `Into<TextStyleProp>`. Using a role resolves at paint/layout time, so
    /// theme typography changes take effect without a rebuild.
    pub fn style(mut self, style: impl Into<TextStyleProp>) -> Self {
        self.style = style.into();
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

    /// Compatibility shim: `.bind_color(signal)` is equivalent to
    /// `.color(signal)` now that `.color(...)` accepts `impl Into<ColorProp>`.
    /// New code should prefer `.color(TextRole::X)` for theme-driven colors
    /// and `.color(signal)` for reactive state.
    pub fn bind_color(self, state: impl Into<ColorProp>) -> Self {
        self.color(state)
    }

    /// Get the current text value (resolves from state if bound).
    pub fn text(&self) -> String {
        self.text.get()
    }

    /// Enable inline markup parsing. When enabled, the text is parsed
    /// as a minimal markdown subset:
    /// - `[label](url)` — inline link
    /// - `*italic*`     — italic run
    /// - `**bold**`     — bold run
    ///
    /// Links are dispatched via [`on_link_click`](Self::on_link_click)
    /// and colored using `theme.colors.text_link`.
    pub fn markup(mut self, enabled: bool) -> Self {
        self.markup = enabled;
        self
    }

    /// Called when an inline link is tapped. Enables markup automatically.
    pub fn on_link_click<F>(mut self, handler: F) -> Self
    where
        F: Fn(&str, &mut EventContext) + 'static,
    {
        self.on_link_click = Some(Rc::new(handler));
        self.markup = true;
        self
    }

    /// Called when an inline link is hovered (enter/leave). Receives
    /// the URL, a `bool` indicating whether the pointer entered (`true`)
    /// or left (`false`), and the widget-local rect of the link span
    /// (so anchoring popups next to the link is cheap). Enables markup
    /// automatically.
    pub fn on_link_hover<F>(mut self, handler: F) -> Self
    where
        F: Fn(&str, bool, Rect, &mut EventContext) + 'static,
    {
        self.on_link_hover = Some(Rc::new(handler));
        self.markup = true;
        self
    }

    /// Hide this text from the accessibility tree. Use this when the
    /// TextWidget is a visual label fragment inside another control
    /// that already owns its accessible name via `set_name` —
    /// otherwise screen readers announce the same string twice
    /// (once for the control, once for the embedded Label node).
    ///
    /// Standalone body text (dialog descriptions, form instructions,
    /// read-only display values) should NOT set this — it stays as a
    /// `Role::Label` node.
    pub fn a11y_hidden(mut self) -> Self {
        self.a11y_hidden = true;
        self
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

        // Wire link dispatch when markup is enabled and at least one
        // link handler is registered. Shares the last_layout cell with
        // the closures so taps can hit-test against the most recently
        // measured spans.
        if self.markup && (self.on_link_click.is_some() || self.on_link_hover.is_some()) {
            let mut handler_set = HandlerSet::new();
            if let Some(on_click) = self.on_link_click.clone() {
                let last_layout = self.last_layout.clone();
                let last_bounds = self.last_bounds.clone();
                handler_set = handler_set.on_tap(move |pt, ctx| {
                    let bounds = last_bounds.get();
                    let local = Point::new(pt.x - bounds.x, pt.y - bounds.y);
                    if let Some(layout) = last_layout.borrow().as_ref()
                        && let Some(HitTarget::Link { url }) = layout.hit_test(local)
                    {
                        on_click(&url, ctx);
                    }
                });
            }

            // Pointer-move handler — wired *unconditionally* when
            // markup is enabled with any link handler. It does two
            // jobs:
            //
            // 1. Updates the cursor to `Pointer` while the pointer is
            //    over a link span, restoring `Default` otherwise. This
            //    is the visual affordance the catalog was missing.
            // 2. Drives `on_link_hover` enter/leave transitions when
            //    that handler is wired, comparing the current
            //    hit-test URL against `hovered_link`.
            //
            // Returns `Ignored` so the gesture arena still receives
            // PointerDown/Up and the on_tap handler keeps firing.
            let last_layout_for_pointer = self.last_layout.clone();
            let last_bounds_for_pointer = self.last_bounds.clone();
            let hovered = self.hovered_link.clone();
            let on_hover = self.on_link_hover.clone();
            handler_set = handler_set.on_pointer_event(move |event, ctx| {
                use fern_core::event::{EventResponse, WidgetEvent};
                match event {
                    WidgetEvent::PointerMove { position } => {
                        let bounds = last_bounds_for_pointer.get();
                        let local =
                            Point::new(position.x - bounds.x, position.y - bounds.y);
                        let layout_ref = last_layout_for_pointer.borrow();
                        let hit = layout_ref.as_ref().and_then(|l| l.hit_test(local));
                        let new_url = match &hit {
                            Some(HitTarget::Link { url }) => Some(url.clone()),
                            _ => None,
                        };

                        // Update cursor based on link hit.
                        if new_url.is_some() {
                            ctx.set_cursor(CursorIcon::Pointer);
                        } else {
                            ctx.set_cursor(CursorIcon::Default);
                        }

                        // Drive enter/leave transitions when an
                        // on_link_hover handler is wired.
                        if let Some(handler) = on_hover.as_ref() {
                            let new_rect = if let Some(url) = new_url.as_ref() {
                                layout_ref
                                    .as_ref()
                                    .and_then(|l| {
                                        l.spans.iter().find_map(|sp| {
                                            if let fern_canvas::text_backend::TextSpanKind::Link { url: u } = &sp.kind
                                                && u == url
                                            {
                                                Some(Rect::new(
                                                    sp.rect[0],
                                                    sp.rect[1],
                                                    sp.rect[2],
                                                    sp.rect[3],
                                                ))
                                            } else {
                                                None
                                            }
                                        })
                                    })
                                    .unwrap_or_else(|| Rect::new(0.0, 0.0, 0.0, 0.0))
                            } else {
                                Rect::new(0.0, 0.0, 0.0, 0.0)
                            };

                            drop(layout_ref);

                            let mut slot = hovered.borrow_mut();
                            if slot.as_deref() != new_url.as_deref() {
                                if let Some(old) = slot.take() {
                                    handler(&old, false, Rect::new(0.0, 0.0, 0.0, 0.0), ctx);
                                }
                                if let Some(u) = new_url {
                                    handler(&u, true, new_rect, ctx);
                                    *slot = Some(u);
                                }
                            }
                        }
                        EventResponse::Ignored
                    }
                    WidgetEvent::PointerLeave => {
                        ctx.set_cursor(CursorIcon::Default);
                        if let Some(handler) = on_hover.as_ref() {
                            let mut slot = hovered.borrow_mut();
                            if let Some(old) = slot.take() {
                                handler(&old, false, Rect::new(0.0, 0.0, 0.0, 0.0), ctx);
                            }
                        }
                        EventResponse::Ignored
                    }
                    _ => EventResponse::Ignored,
                }
            });

            ctx.apply_self_handlers(handler_set);
        }

        Vec::new()
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        let text = self.text.get();
        let style = self.style.resolve(&ctx.theme.typography);
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

        // Add a small epsilon to the proposal width before passing it as
        // max_width. The same epsilon is applied in Canvas::draw_text /
        // draw_paragraph so both measurement and paint produce the same
        // TypesetterBridge cache key, avoiding duplicate cache entries
        // and inconsistent truncation from float precision loss in the
        // scale_factor roundtrip (logical → physical → logical).
        let max_width = proposal.width.map(|w| w + 0.5);

        // Markup path: only reachable in Wrap mode. The backend parses
        // the source internally and returns a TextLayout whose `spans`
        // field carries per-run rects (including links) that we stash
        // for hit-testing during event dispatch.
        if self.markup {
            let layout = match max_width {
                Some(w) => backend.layout_paragraph_markup(&text, &style, w, self.max_lines),
                None => backend.layout_single_line_markup(&text, &style, None),
            };
            let size = Size::new(layout.width, layout.height);
            *self.last_layout.borrow_mut() = Some(layout);
            return size;
        }

        match self.overflow {
            TextOverflow::Wrap => match max_width {
                Some(w) => {
                    let layout = backend.layout_paragraph(&text, &style, w, self.max_lines);
                    Size::new(layout.width, layout.height)
                }
                None => {
                    // Unconstrained width: no basis for wrapping, so measure
                    // as a single line.
                    let layout = backend.layout_single_line(&text, &style, None);
                    Size::new(layout.width, layout.height)
                }
            },
            TextOverflow::Ellipsis(EllipsisMode::Trailing) => {
                // text-typeset truncates with trailing "…" when a max_width
                // is supplied — let it do the work.
                let layout = backend.layout_single_line(&text, &style, max_width);
                Size::new(layout.width, layout.height)
            }
            TextOverflow::Ellipsis(mode) => {
                // Middle / Leading: compute the truncated display string
                // first, then measure it unconstrained.
                let Some(max_w) = max_width else {
                    let layout = backend.layout_single_line(&text, &style, None);
                    return Size::new(layout.width, layout.height);
                };
                let truncated = fern_canvas::ellipsis::ellipsize(
                    &text,
                    &style,
                    max_w,
                    mode,
                    &mut *backend,
                );
                let layout = backend.layout_single_line(&truncated, &style, None);
                Size::new(layout.width, layout.height)
            }
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        // Stash bounds so the on_tap / on_pointer_event closures can
        // convert window-local event positions into widget-local
        // before hit-testing against the layout's spans.
        self.last_bounds.set(bounds);

        let text = self.text.get();
        let color = self.color.resolve(ctx.theme);
        let style = self.style.resolve(&ctx.theme.typography);

        // Markup path: re-measure through the markup pipeline (the
        // backend's cache makes this a no-op after the size pass) and
        // draw with per-span coloring so link glyphs pick up the
        // theme's `text_link` token.
        if self.markup {
            let Some(backend_rc) = canvas.text_backend() else {
                return;
            };
            let layout = {
                let mut backend = backend_rc.borrow_mut();
                match self.overflow {
                    TextOverflow::Wrap => backend.layout_paragraph_markup(
                        &text,
                        &style,
                        (bounds.width + 0.5).max(0.0),
                        self.max_lines,
                    ),
                    _ => backend.layout_single_line_markup(
                        &text,
                        &style,
                        Some(bounds.width + 0.5),
                    ),
                }
            };
            let link_color = ctx.theme.colors.text_link;
            canvas.draw_text_layout_markup(
                &layout,
                fern_canvas::Point::new(bounds.x, bounds.y),
                color,
                link_color,
            );
            // Keep the cached layout in sync so tap hit-testing sees
            // the same rects that were painted.
            *self.last_layout.borrow_mut() = Some(layout);
            return;
        }

        match self.overflow {
            TextOverflow::Wrap => {
                canvas.draw_paragraph(&text, bounds, &style, color, self.max_lines);
            }
            TextOverflow::Ellipsis(EllipsisMode::Trailing) => {
                canvas.draw_text(&text, bounds, &style, color);
            }
            TextOverflow::Ellipsis(mode) => {
                // Produce the truncated display string via the canvas's
                // backend and hand it to draw_text.
                let truncated = match canvas.text_backend() {
                    Some(backend) => fern_canvas::ellipsis::ellipsize(
                        &text,
                        &style,
                        bounds.width,
                        mode,
                        &mut *backend.borrow_mut(),
                    ),
                    None => text.clone(),
                };
                canvas.draw_text(&truncated, bounds, &style, color);
            }
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        if self.a11y_hidden {
            return;
        }
        let text = self.text.get();
        builder.set_role(fern_core::accesskit::Role::Label);
        builder.set_name(&text);

        // Markup mode: surface inline links as synthetic `Role::Link`
        // children so screen readers can focus them individually. The
        // rects from `last_layout` are in widget-local space and carry
        // enough information to identify each unique URL.
        if self.markup
            && let Some(layout) = self.last_layout.borrow().as_ref()
        {
            // Dedupe by (url, byte_range.start): a link that wraps
            // across two lines produces two LaidOutSpan entries sharing
            // the same URL and byte range, but we only want one
            // accessible node per source link.
            let mut seen: Vec<(String, usize)> = Vec::new();
            for span in &layout.spans {
                if let fern_canvas::text_backend::TextSpanKind::Link { url } = &span.kind {
                    let key = (url.clone(), span.byte_range.start);
                    if seen.iter().any(|k| k == &key) {
                        continue;
                    }
                    seen.push(key.clone());
                    // Use the byte offset as the element_id so the
                    // synthetic NodeId is stable across re-layouts.
                    let element_id = span.byte_range.start as u64;
                    let label = text
                        .get(span.byte_range.clone())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| url.clone());
                    builder.push_link_child(element_id, label, url.clone());
                }
            }
        }
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
