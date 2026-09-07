// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! TextWidget — a leaf widget that renders a localized text string.
//!
//! `TextWidget` is the building block for every visible label in the framework.
//! It delegates measurement and rasterization to the `TextBackend` and supports
//! three overflow modes: [`TextOverflow::Wrap`] (default — grows vertically),
//! [`TextOverflow::Ellipsis`] with trailing, middle, or leading truncation, and
//! a minimal markup subset (`[label](url)`, `*italic*`, `**bold**`) with
//! per-link click/hover dispatch.
//!
//! Text and color accept either static values or reactive `Signal`/`Prop` bindings.
//! The default color role is [`TextRole::Primary`], resolved against the active
//! theme at paint time, so theme switches update text color without any explicit
//! binding or rebuild.
//!
//! # Accessibility
//!
//! A `TextWidget` is reviewable by default: it emits AccessKit text runs —
//! one per visual line, with a position and an advance for every character
//! — so a screen reader's review cursor can walk it, a braille cell can be
//! routed to the word under it, and `AXBoundsForRange` and its Windows and
//! Linux equivalents can answer. Runs are excluded from object navigation
//! by `accesskit_consumer`'s own filter, so they add no stops to anyone's
//! traversal and no targets to any hit test.
//!
//! Geometry is always present: real when a text backend measured the label,
//! and a zero-width box at its leading edge when none did. Never absent —
//! `Range::bounding_boxes()` discards every box it has collected on the
//! first run missing any of bounds, direction, positions or widths, so one
//! geometry-less run would take the whole label's geometry with it.
//!
//! A label *inside* a control that owns the same accessible name must call
//! [`a11y_hidden`](TextWidget::a11y_hidden), or a reader hears the string
//! twice. Standalone body text should not.
//!
//! Single-line / ellipsis text opts into shrink by default: an over-constrained
//! stack compresses the label down to the ellipsis-glyph width before the label
//! overflows. Call [`no_shrink`](TextWidget::no_shrink) to restore rigid behavior,
//! or [`min_shrink_width`](TextWidget::min_shrink_width) to set a custom floor.
//! Wrap-mode text is height-variable and therefore always rigid; opt it into
//! compression with [`Shrinkable`](crate::primitives::Shrinkable).
//!
//! ```rust
//! # use teksilo_widgets::primitives::TextWidget;
//! # use teksilo_i18n::lit;
//! // Single-line label that truncates with a trailing ellipsis if too narrow:
//! let _w = TextWidget::new(lit!("Save document")).single_line();
//! ```
//!
//! ## Touch and pen
//!
//! Inline links are reachable by a plain tap: the handler at `on_tap` follows
//! the link run under the press with no modifier of any kind. (The touch
//! inventory recorded this file as Ctrl-gated and therefore unreachable by
//! touch; that gate is `rich_text/mouse.rs`'s — `modifiers.command() ||
//! read_only` — and does not exist here.) The hover cursor over a link run is
//! a mouse and pen affordance and costs a finger nothing.
//!
//! A link run is text-height and its size is constrained by the line height of
//! the text around it, which is exactly WCAG 2.2 SC 2.5.8's *inline* exception,
//! so it is not raised to the 24 dp floor and is not reported as a target
//! region.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::ellipsis::{ELLIPSIS, Ellipsized, ellipsize_ranges};
use teksilo_canvas::text_backend::{
    CharGeom, HitTarget, LineEnd, LineTruncation, TextDirection, TextGeometry, TextLayout,
    TextLine, TextLineSegment,
};
use teksilo_canvas::{Canvas, EllipsisMode, Point, Rect, Size, SizeProposal, TextOverflow};

use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accessibility::text_runs::{
    RetainedText, TextGeometryHandle, TextRunSource, push_text_runs,
};
use teksilo_core::color_prop::{ColorProp, TextStyleProp};
use teksilo_core::signal::Prop;
use teksilo_core::widget::{CursorIcon, EventContext, LayoutContext, PaintContext, Widget};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_i18n::LocalizedString;
use teksilo_tokens::TextRole;

/// A leaf widget that renders a localized text string.
///
/// See the [module documentation](self) for the full feature description.
/// Construct with [`TextWidget::new`] and chain builder methods for color,
/// style, overflow mode, and optional markup/link dispatch.
/// Closure type for link click/hover dispatch.
type LinkClickHandler = Rc<dyn Fn(&str, &mut EventContext)>;
type LinkHoverHandler = Rc<dyn Fn(&str, bool, Rect, &mut EventContext)>;

pub struct TextWidget {
    text: Prop<String>,
    /// Foreground color. Defaults to [`TextRole::Primary`], which the paint
    /// pass resolves against the current theme — so `TextWidget::new("...")`
    /// follows theme switches without any explicit binding.
    color: ColorProp,
    /// Text style. Defaults to [`TextStyleRole::Body`](teksilo_tokens::TextStyleRole);
    /// resolved against the current typography tokens every time the widget
    /// is laid out or painted, so theme switches update font metrics without
    /// a rebuild.
    style: TextStyleProp,
    overflow: TextOverflow,
    /// Whether single-line / ellipsis text reports a shrink weight so an
    /// over-constrained stack truncates it (down to [`Self::min_shrink_width`]) rather
    /// than overflowing. Default `true`. Ignored in `Wrap` mode (wrap text is
    /// height-variable and opts into shrink via `Shrinkable` instead).
    shrink: bool,
    /// Explicit compression floor for ellipsis text. `None` measures the
    /// width of the ellipsis glyph at layout time.
    min_shrink_width: Option<f32>,
    max_lines: Option<usize>,
    text_backend: Option<Rc<RefCell<dyn teksilo_canvas::TextBackend>>>,
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
    /// Currently-hovered link URL (shared with the pointer-event
    /// closure). Used to detect enter/leave transitions between link
    /// spans inside a single widget.
    hovered_link: Rc<RefCell<Option<String>>>,
    /// What the last placement measured: the text as painted, the
    /// geometry behind it, and the box it occupies — everything the
    /// accessibility pass needs to describe this label one character at a
    /// time, and everything a control that paints through a *hidden*
    /// `TextWidget` needs to describe it on the label's behalf (see
    /// [`geometry_handle`](TextWidget::geometry_handle)).
    ///
    /// Written from `place_children`, the first point at which the final
    /// width is known: `layout_response` runs speculatively, more than
    /// once, at proposals the widget may not end up with.
    retained: TextGeometryHandle,
    /// Absolute line box of every text run the last accessibility pass
    /// emitted. An assistive technology asking to scroll a run into view
    /// names the *run*, not the label, so the reveal needs its line.
    run_lines: Rc<RefCell<Vec<(teksilo_core::accesskit::NodeId, Rect)>>>,
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
    /// change) or from `lit!("…")` for genuinely
    /// non-translated strings.
    pub fn new(text: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = text.into();
        Self {
            text: Prop::from(ls),
            color: ColorProp::TextRole(TextRole::Primary),
            style: TextStyleProp::default(),
            overflow: TextOverflow::default(),
            shrink: true,
            min_shrink_width: None,
            max_lines: None,
            text_backend: None,
            markup: false,
            on_link_click: None,
            on_link_hover: None,
            last_layout: Rc::new(RefCell::new(None)),
            retained: Rc::new(RefCell::new(None)),
            run_lines: Rc::new(RefCell::new(Vec::new())),
            hovered_link: Rc::new(RefCell::new(None)),
            a11y_hidden: false,
        }
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

    /// Set the text style. Accepts a raw `TextStyle`, a
    /// [`TextStyleRole`](teksilo_tokens::TextStyleRole), or any value implementing
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

    /// Override the compression floor for single-line / ellipsis text — the
    /// narrowest width an over-constrained stack may shrink this label to
    /// before truncating stops. Defaults to the ellipsis-glyph width.
    pub fn min_shrink_width(mut self, min: f32) -> Self {
        self.min_shrink_width = Some(min.max(0.0));
        self
    }

    /// Opt this label out of native shrink: it reports a rigid size and
    /// overflows (rather than truncating) when its stack is over-constrained.
    pub fn no_shrink(mut self) -> Self {
        self.shrink = false;
        self
    }

    /// Cap the paragraph at `n` lines when wrapping. Only meaningful
    /// in [`TextOverflow::Wrap`] mode — ignored for ellipsis modes.
    /// Lines beyond the cap are silently dropped.
    pub fn max_lines(mut self, n: usize) -> Self {
        self.max_lines = Some(n);
        self
    }

    /// Override the text backend used for measurement and rasterization.
    /// In normal app code the framework provides the backend automatically;
    /// this method is used by headless tests that inject a `MockTextBackend`.
    pub fn text_backend(mut self, backend: Rc<RefCell<dyn teksilo_canvas::TextBackend>>) -> Self {
        self.text_backend = Some(backend);
        self
    }

    /// Set the text content. Accepts a static `String`/`&str` or a reactive
    /// `Signal<String>` / `Prop<String>` (resolved and re-rendered on change).
    pub fn text(mut self, state: impl Into<Prop<String>>) -> Self {
        self.text = state.into();
        self
    }

    /// Get the current text value (resolves from state if bound).
    pub fn resolved_text(&self) -> String {
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

    /// A handle onto what this label last measured.
    ///
    /// For the controls that own their accessible name but paint their
    /// text through a hidden `TextWidget` — `Badge`, `GroupHeader` — and
    /// so have to emit that text's runs from their *own* node. The handle
    /// is empty until the label has been placed, and the geometry it
    /// carries is in window space, so the borrower reports it as absolute.
    ///
    /// The borrower must place the donor rigidly at its own origin: a
    /// move re-places absolute rects by the *borrower's* delta, so a
    /// composite that repositions its donor independently should register
    /// local rects instead.
    pub fn geometry_handle(&self) -> TextGeometryHandle {
        self.retained.clone()
    }
}

/// Map a laid-out ellipsis display string back onto the source it stands for.
///
/// Middle and leading ellipsis paint a *different string* from the
/// source: `"Lorem…amet"` for `"Lorem ipsum dolor sit amet"`. The
/// geometry the backend returns indexes that display string, but a reader
/// must be able to review the whole source — so the kept spans keep their
/// real measurements, re-labelled with source offsets, and the elided
/// span is left uncovered for the emitter to anchor at the ellipsis glyph
/// it was replaced by.
///
/// Yields no segments for a right-to-left line: the kept spans would have
/// to be re-derived from the right edge, and a degenerate label is honest
/// where a wrong rectangle is not.
fn remap_ellipsized(source: &str, display: &TextGeometry, cut: &Ellipsized) -> TextGeometry {
    /// One contiguous stretch of source characters kept from the display.
    struct Bucket {
        bytes: std::ops::Range<usize>,
        chars: std::ops::Range<usize>,
        geometry: Vec<CharGeom>,
    }

    let head_len = cut.head.len();
    let ellipsis_len = ELLIPSIS.len_utf8();
    let display_text = cut.display.as_str();
    // How many source characters the ellipsis stands for, so the kept
    // tail's char indices carry on from the head's.
    let elided_chars = source
        .get(cut.elided.clone())
        .map(|s| s.chars().count())
        .unwrap_or(0);

    let mut lines = Vec::with_capacity(display.lines.len());
    for line in &display.lines {
        let mut segments: Vec<TextLineSegment> = Vec::new();
        let mut ellipsis_x: Option<f32> = None;
        let mut usable = true;

        for segment in &line.segments {
            if segment.direction == TextDirection::RightToLeft {
                usable = false;
                break;
            }
            let Some(slice) = display_text.get(segment.byte_range.clone()) else {
                usable = false;
                break;
            };

            let mut buckets: Vec<Bucket> = Vec::new();
            let mut display_char = segment.char_range.start;
            for ((offset, ch), geometry) in slice.char_indices().zip(&segment.characters) {
                let display_byte = segment.byte_range.start + offset;
                let in_head = display_byte < head_len;
                if !in_head && display_byte < head_len + ellipsis_len {
                    ellipsis_x.get_or_insert(segment.rect[0] + geometry.position);
                    display_char += 1;
                    continue;
                }
                let (source_byte, source_char) = if in_head {
                    (display_byte, display_char)
                } else {
                    (
                        cut.tail.start + (display_byte - head_len - ellipsis_len),
                        display_char - 1 + elided_chars,
                    )
                };
                let end_byte = source_byte + ch.len_utf8();
                match buckets.last_mut() {
                    Some(bucket) if bucket.bytes.end == source_byte => {
                        bucket.bytes.end = end_byte;
                        bucket.chars.end = source_char + 1;
                        bucket.geometry.push(*geometry);
                    }
                    _ => buckets.push(Bucket {
                        bytes: source_byte..end_byte,
                        chars: source_char..source_char + 1,
                        geometry: vec![*geometry],
                    }),
                }
                display_char += 1;
            }

            for bucket in buckets {
                let base = bucket.geometry.first().map(|c| c.position).unwrap_or(0.0);
                let extent = bucket
                    .geometry
                    .last()
                    .map(|c| c.position + c.width - base)
                    .unwrap_or(0.0);
                segments.push(TextLineSegment {
                    byte_range: bucket.bytes,
                    char_range: bucket.chars,
                    direction: segment.direction,
                    rect: [
                        segment.rect[0] + base,
                        segment.rect[1],
                        extent,
                        segment.rect[3],
                    ],
                    characters: bucket
                        .geometry
                        .iter()
                        .map(|c| CharGeom {
                            position: c.position - base,
                            width: c.width,
                        })
                        .collect(),
                });
            }
        }

        if !usable {
            segments.clear();
        }
        lines.push(TextLine {
            index: line.index,
            byte_range: 0..source.len(),
            char_range: 0..source.chars().count(),
            rect: line.rect,
            baseline: line.baseline,
            caret_x: line.caret_x,
            segments,
            end: LineEnd::EndOfText,
            truncation: ellipsis_x.map(|x| LineTruncation {
                ellipsis_x: x,
                ellipsis_width: 0.0,
            }),
        });
    }

    TextGeometry {
        lines,
        dropped_lines: display.dropped_lines,
        source_len: source.len(),
        rendered_text: None,
        links: Vec::new(),
    }
}

impl Widget for TextWidget {
    fn build(
        &mut self,
        ctx: &mut teksilo_core::build_context::BuildContext,
    ) -> Vec<teksilo_core::widget_id::WidgetId> {
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.text.register_if_bound(
            self_id,
            registry,
            teksilo_core::binding::BindingLevel::Relayout,
        );
        // A text change is a relayout AND a semantic change. `Relayout`
        // alone never dirties the accessibility tree — deliberately, so a
        // per-frame animation does not re-walk it — so a label bound to a
        // signal would re-shape, repaint, and go on announcing the old
        // string.
        self.text.register_if_bound(
            self_id,
            registry,
            teksilo_core::binding::BindingLevel::AccessibilityOnly,
        );
        self.color.register_if_bound(
            self_id,
            registry,
            teksilo_core::binding::BindingLevel::RepaintOnly,
        );

        // Wire link dispatch when markup is enabled and at least one
        // link handler is registered. Shares the last_layout cell with
        // the closures so taps can hit-test against the most recently
        // measured spans.
        let mut handler_set = HandlerSet::new();

        // Reveal-on-request. An assistive technology scrolling a run into
        // view names the *run*, not the label, and every adapter offers
        // the action on any node that carries text ranges whether or not
        // it was advertised — so handle it, and do not advertise it: an
        // advertised `ScrollIntoView` puts a scroll pattern on every
        // label on Windows and macOS.
        //
        // `SetTextSelection` is likewise offered everywhere and is
        // meaningless on a label; taking it and doing nothing is the
        // honest answer, and keeps a stray request from bubbling to an
        // ancestor that would act on it.
        {
            let run_lines = self.run_lines.clone();
            let retained = self.retained.clone();
            handler_set = handler_set.on_access_action_request(move |action, target, data, ctx| {
                use teksilo_core::accesskit::{Action, ActionData, ScrollHint};
                use teksilo_core::event::{EventResponse, ScrollMotion};
                match action {
                    Action::ScrollIntoView => {
                        let rect = run_lines
                            .borrow()
                            .iter()
                            .find(|(id, _)| *id == target)
                            .map(|(_, rect)| *rect)
                            .or_else(|| retained.borrow().as_ref().map(|r| r.bounds));
                        let Some(rect) = rect else {
                            return EventResponse::Ignored;
                        };
                        match data {
                            Some(ActionData::ScrollHint(
                                ScrollHint::TopLeft | ScrollHint::TopEdge,
                            )) => ctx.ensure_visible_aligned(rect, 0.0, ScrollMotion::Instant),
                            Some(ActionData::ScrollHint(
                                ScrollHint::BottomRight | ScrollHint::BottomEdge,
                            )) => ctx.ensure_visible_aligned(rect, 1.0, ScrollMotion::Instant),
                            _ => ctx.ensure_visible(rect),
                        }
                        EventResponse::Handled
                    }
                    Action::SetTextSelection => EventResponse::Handled,
                    _ => EventResponse::Ignored,
                }
            });
        }

        if self.markup && (self.on_link_click.is_some() || self.on_link_hover.is_some()) {
            if let Some(on_click) = self.on_link_click.clone() {
                let last_layout = self.last_layout.clone();
                handler_set = handler_set.on_tap(move |event, ctx| {
                    // `event.position` is already widget-local.
                    let local = event.position;
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
            let hovered = self.hovered_link.clone();
            let on_hover = self.on_link_hover.clone();
            handler_set = handler_set.on_pointer_event(move |event, ctx| {
                use teksilo_core::event::{EventResponse, WidgetEvent};
                match event {
                    WidgetEvent::PointerMove { position } => {
                        // `position` is already widget-local.
                        let local = *position;
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
                                            if let teksilo_canvas::text_backend::TextSpanKind::Link {
                                                url: u,
                                            } = &sp.kind
                                                && u == url
                                            {
                                                Some(Rect::new(
                                                    sp.rect[0], sp.rect[1], sp.rect[2], sp.rect[3],
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
        }

        ctx.apply_self_handlers(handler_set);
        Vec::new()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
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
            return (Size::new(w, height)).into();
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
            // Branch on the overflow mode, exactly as `paint` does. Keying
            // on whether a width was proposed instead meant an `Ellipsis`
            // markup label measured as a wrapped paragraph and painted as
            // one clipped line, so it reserved height for lines that were
            // never drawn.
            let layout = match self.overflow {
                TextOverflow::Wrap => match max_width {
                    Some(w) => backend.layout_paragraph_markup(&text, &style, w, self.max_lines),
                    None => backend.layout_single_line_markup(&text, &style, None),
                },
                _ => backend.layout_single_line_markup(&text, &style, max_width),
            };
            return Size::new(layout.width, layout.height).into();
        }

        let is_ellipsis = matches!(self.overflow, TextOverflow::Ellipsis(_));

        let size = match self.overflow {
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
            TextOverflow::Ellipsis(mode) => match max_width {
                // Middle / Leading: compute the truncated display string
                // first, then measure it unconstrained.
                None => {
                    let layout = backend.layout_single_line(&text, &style, None);
                    Size::new(layout.width, layout.height)
                }
                Some(max_w) => {
                    let truncated = teksilo_canvas::ellipsis::ellipsize(
                        &text,
                        &style,
                        max_w,
                        mode,
                        &mut *backend,
                    );
                    let layout = backend.layout_single_line(&truncated, &style, None);
                    Size::new(layout.width, layout.height)
                }
            },
        };

        // Single-line / ellipsis labels opt into shrink (they are height-stable
        // — truncating never changes their line height). An over-constrained
        // stack compresses them down to the ellipsis-glyph width (or the
        // caller's `min_shrink_width`) and they ellipsize instead of
        // overflowing. Wrap text stays rigid; opt it in with `Shrinkable`.
        if is_ellipsis && self.shrink {
            let min_w = self
                .min_shrink_width
                .unwrap_or_else(|| backend.layout_single_line("…", &style, None).width)
                .min(size.width);
            teksilo_core::widget::LayoutResponse::shrinkable(
                size,
                Size::new(min_w, size.height),
                1.0,
            )
        } else {
            size.into()
        }
    }

    /// Shape once more at the final width, and keep what came back.
    ///
    /// This is the only pass that sees the width the label actually got:
    /// `layout_response` runs speculatively, more than once, at proposals
    /// the parent may not honour. The accessibility tree needs the real
    /// one — a label that wraps at 200 dp has a different set of lines,
    /// and therefore of text runs, from the same label at 400 dp.
    ///
    /// The call mirrors `paint`'s exactly, including its `+ 0.5` width
    /// epsilon, so both land on the same backend cache entry. Under a
    /// scale transform (a zoomed `SceneView`, a `Scale` wrapper) the
    /// ambient raster scale differs between this pass and paint, so the
    /// two hit different entries: the metrics are identical — the cache
    /// keys on raster scale only for glyph density — and the geometry is
    /// right either way, at the cost of one extra shaping per (text,
    /// scale).
    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [teksilo_core::widget::WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let text = self.text.get();
        let base_direction = match ctx.layout_direction {
            teksilo_core::environment::LayoutDirection::RightToLeft => {
                teksilo_core::accesskit::TextDirection::RightToLeft
            }
            _ => teksilo_core::accesskit::TextDirection::LeftToRight,
        };
        let style = self.style.resolve(&ctx.theme.typography);
        let Some(backend) = self.text_backend.as_ref().or(ctx.text_backend) else {
            // Painted without a measuring backend. The label is still
            // announced and still reviewable; its characters simply have
            // no extents, which the emitter reports as a zero-width box at
            // the label's leading edge rather than as no box at all.
            *self.retained.borrow_mut() = Some(RetainedText {
                text,
                geometry: None,
                bounds,
                base_direction,
            });
            return;
        };
        let mut backend = backend.borrow_mut();
        let max_width = bounds.width + 0.5;

        let (layout, retained_text, remapped) = if self.markup {
            let layout = match self.overflow {
                TextOverflow::Wrap => backend.layout_paragraph_markup(
                    &text,
                    &style,
                    max_width.max(0.0),
                    self.max_lines,
                ),
                _ => backend.layout_single_line_markup(&text, &style, Some(max_width)),
            };
            // Markup ranges index the rendered text, so that is what the
            // label announces and what a reader reviews — not the source
            // with its brackets and parentheses in it.
            let rendered = layout
                .geometry
                .as_deref()
                .and_then(|g| g.rendered_text.clone())
                .unwrap_or_else(|| text.clone());
            (Some(layout), rendered, None)
        } else {
            match self.overflow {
                TextOverflow::Wrap => {
                    let layout =
                        backend.layout_paragraph(&text, &style, max_width.max(0.0), self.max_lines);
                    (Some(layout), text.clone(), None)
                }
                TextOverflow::Ellipsis(EllipsisMode::Trailing) => {
                    let layout = backend.layout_single_line(&text, &style, Some(max_width));
                    (Some(layout), text.clone(), None)
                }
                TextOverflow::Ellipsis(mode) => {
                    // Middle and leading ellipsis paint a different string
                    // from the source, so the geometry has to be mapped
                    // back onto it before anyone can review the label.
                    let cut = ellipsize_ranges(&text, &style, bounds.width, mode, &mut *backend);
                    let layout = backend.layout_single_line(&cut.display, &style, None);
                    let remapped = layout
                        .geometry
                        .as_deref()
                        .map(|g| Rc::new(remap_ellipsized(&text, g, &cut)));
                    (Some(layout), text.clone(), remapped)
                }
            }
        };

        let Some(layout) = layout else { return };
        let geometry = remapped.or_else(|| layout.geometry.clone());
        if self.markup {
            // Hit-testing reads the spans the label was actually placed
            // at, so a tap lands on the right link before the first paint.
            *self.last_layout.borrow_mut() = Some(layout);
        }
        *self.retained.borrow_mut() = Some(RetainedText {
            text: retained_text,
            geometry,
            bounds,
            base_direction,
        });
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let text = self.text.get();
        let color = self.color.resolve(ctx.theme, ctx.effective_enabled);
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
                    _ => backend.layout_single_line_markup(&text, &style, Some(bounds.width + 0.5)),
                }
            };
            let link_color = ctx.theme.colors.text_link;
            canvas.draw_text_layout_markup(
                &layout,
                teksilo_canvas::Point::new(bounds.x, bounds.y),
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
                    Some(backend) => teksilo_canvas::ellipsis::ellipsize(
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
        builder.set_role(teksilo_core::accesskit::Role::Label);

        // A `Role::Label` with `Role::TextRun` children is what makes a
        // label reviewable by character, word and line — and braille-
        // routable, and trackable by a magnifier. The runs are excluded
        // from object navigation by `accesskit_consumer`'s own filter, so
        // they add stops to no reader's tab order and targets to no
        // hit-test.
        let retained = self.retained.borrow();
        let unplaced;
        let source = match retained.as_ref() {
            Some(placed) => match placed.geometry.as_deref() {
                // Rects are reported in the label's own space; the builder
                // translates them once the walker has written the label's
                // window-space box.
                Some(geometry) => {
                    TextRunSource::from_geometry(&placed.text, geometry, Point::ZERO, 0)
                        .with_base_direction(placed.base_direction)
                }
                None => TextRunSource::flat(&placed.text, 0)
                    .with_fallback_rect(Rect::new(
                        0.0,
                        0.0,
                        placed.bounds.width,
                        placed.bounds.height,
                    ))
                    .with_base_direction(placed.base_direction),
            },
            // Never placed — a name probe, or a tree asked for its
            // accessibility before its first layout.
            None => {
                unplaced = self.text.get();
                TextRunSource::flat(&unplaced, 0)
            }
        };

        let emission = push_text_runs(builder, None, &source);
        // The name must be byte-identical to the runs' concatenation: the
        // consumer derives the document text from the runs, and every
        // divergence is a place where what a reader reviews and what it
        // announces disagree.
        builder.set_name(&emission.value);

        let origin = retained
            .as_ref()
            .map(|r| r.bounds.origin())
            .unwrap_or(Point::ZERO);
        let mut run_lines = self.run_lines.borrow_mut();
        run_lines.clear();
        for run in &emission.runs {
            let Some(line) = source
                .lines
                .iter()
                .find(|line| line.byte_range.contains(&run.byte_range.start))
                .or_else(|| source.lines.last())
            else {
                continue;
            };
            run_lines.push((
                run.id,
                Rect::new(
                    origin.x + line.rect.x,
                    origin.y + line.rect.y,
                    line.rect.width,
                    line.rect.height,
                ),
            ));
        }
        drop(run_lines);

        // Markup links become `Role::Link` children beside the runs, so a
        // reader can reach each one as an object. Identified and labelled
        // by their *rendered* offsets: the source offsets shift when the
        // label rewraps, and slicing the source with a rendered range gave
        // "[doc" for a link labelled "docs".
        if self.markup
            && let Some(geometry) = retained.as_ref().and_then(|r| r.geometry.as_deref())
        {
            for link in &geometry.links {
                let label = source
                    .text
                    .get(link.rendered_byte_range.clone())
                    .unwrap_or(&link.url);
                builder.push_link_child(
                    link.rendered_byte_range.start as u64,
                    label.to_string(),
                    link.url.clone(),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use teksilo_canvas::MockTextBackend;
    use teksilo_core::signal::Signal;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_i18n::lit;

    fn tree_with_mock_backend() -> WidgetTree {
        WidgetTree::new().with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())))
    }

    #[test]
    fn text_renders_state_value() {
        let text = Signal::new("Hello".to_string());
        let mut tree = WidgetTree::new();
        let w = tree.add(TextWidget::new(lit!("")).text(text.clone()));
        text.bind_to(
            w,
            tree.binding_registry(),
            teksilo_core::binding::BindingLevel::Relayout,
        );
        tree.layout(SizeProposal::exact(200.0, 40.0));

        assert_eq!(tree.text_content(w), Some("Hello".to_string()));
    }

    #[test]
    fn text_updates_on_state_change() {
        let text = Signal::new("Hello".to_string());
        let mut tree = WidgetTree::new();
        let w = tree.add(TextWidget::new(lit!("")).text(text.clone()));
        text.bind_to(
            w,
            tree.binding_registry(),
            teksilo_core::binding::BindingLevel::Relayout,
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
        let w = TextWidget::new(lit!("Hello"));
        assert_eq!(w.overflow, TextOverflow::Wrap);
    }

    #[test]
    fn ellipsis_label_shrinks_to_fit_in_narrow_stack() {
        use crate::primitives::hstack::HStack;
        let mut tree = tree_with_mock_backend();
        // "hello world" = 11 chars × 8px = 88px natural width.
        let label = tree.add(TextWidget::new(lit!("hello world")).single_line());
        let _stack = tree.add(HStack::new().add_child(label));
        tree.layout(SizeProposal::exact(40.0, 20.0));
        // Single-line text opts into shrink: it compresses to fit the 40px
        // stack and ellipsizes, instead of overflowing to 88px.
        assert!(
            (tree.bounds(label).width - 40.0).abs() < 1.0,
            "ellipsis label should shrink to ~40, got {}",
            tree.bounds(label).width
        );
    }

    #[test]
    fn no_shrink_label_overflows_in_narrow_stack() {
        use crate::primitives::hstack::HStack;
        let mut tree = tree_with_mock_backend();
        let label = tree.add(
            TextWidget::new(lit!("hello world"))
                .single_line()
                .no_shrink(),
        );
        let _stack = tree.add(HStack::new().add_child(label));
        tree.layout(SizeProposal::exact(40.0, 20.0));
        // Opted out → keeps its full 88px and overflows.
        assert!(
            (tree.bounds(label).width - 88.0).abs() < 1.0,
            "no_shrink label should overflow at 88, got {}",
            tree.bounds(label).width
        );
    }

    #[test]
    fn wrap_text_does_not_shrink_natively() {
        use crate::primitives::hstack::HStack;
        let mut tree = tree_with_mock_backend();
        // Wrap mode (default) is height-variable → rigid; overflows rather than
        // shrinking. (Opt in via `Shrinkable`.)
        let label = tree.add(TextWidget::new(lit!("hello world")));
        let _stack = tree.add(HStack::new().add_child(label));
        tree.layout(SizeProposal::exact(40.0, 20.0));
        assert!(
            (tree.bounds(label).width - 88.0).abs() < 1.0,
            "wrap label should not natively shrink, got {}",
            tree.bounds(label).width
        );
    }

    #[test]
    fn wrap_grows_vertically_in_narrow_proposal() {
        // MockTextBackend: 8px/char, 16px line height. "one two three four"
        // = 18 bytes × 8 = 144px wide single-line. At max_width 50 it
        // should wrap across several lines.
        let mut tree = tree_with_mock_backend();
        let w = tree.add(TextWidget::new(lit!("one two three four")));
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
        let w = tree.add(TextWidget::new(lit!("one two three")));
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
        let w = tree.add(TextWidget::new(lit!("one two three four five six seven")).max_lines(2));
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
            TextWidget::new(lit!("a very long piece of text"))
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
            TextWidget::new(lit!("abcdefghijklmnop"))
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
            TextWidget::new(lit!("abcdefghijklmnop"))
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
        let a = TextWidget::new(lit!("hi")).single_line();
        let b =
            TextWidget::new(lit!("hi")).overflow(TextOverflow::Ellipsis(EllipsisMode::Trailing));
        assert_eq!(a.overflow, b.overflow);
    }
}
