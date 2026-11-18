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
//! touch; that gate was `rich_text/mouse.rs`'s — `modifiers.command() ||
//! read_only`, since widened to `kind.is_direct() || read_only ||
//! command_held` — and never existed here.) The hover cursor over a link run
//! is a mouse and pen affordance and costs a finger nothing.
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
    TextLayoutRequest, TextLine, TextLineSegment,
};
use teksilo_canvas::{
    Canvas, EllipsisMode, Point, Rect, Size, SizeProposal, TextAlign, TextOverflow,
};

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

/// Closure type for link click dispatch.
/// Closure type for link hover dispatch.
/// A leaf widget that renders a localized text string.
///
/// See the [module documentation](self) for the full feature description.
/// Construct with [`TextWidget::new`] and chain builder methods for color,
/// style, overflow mode, and optional markup/link dispatch.
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
    /// Where each line sits across the widget's width. See
    /// [`Self::text_align`].
    align: TextAlign,
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
            align: TextAlign::Leading,
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

    /// Place each line of the text across the widget's width:
    /// [`Leading`](TextAlign::Leading) (the default), [`Center`](TextAlign::Center)
    /// or [`Trailing`](TextAlign::Trailing).
    ///
    /// Every line of a wrapped text is placed on its own, so a centred
    /// sentence on two lines has both lines centred, and a single line
    /// narrower than the widget is placed the same way. Leading and trailing
    /// follow the layout direction: in a right-to-left layout `Leading` is
    /// the right edge. The widget's measured size does not change: a wrapped
    /// text is as wide as its widest line, and its lines are placed within
    /// whatever width it is given.
    pub fn text_align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
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

    /// Whether [`Self::text_align`] moves any line from where the backend
    /// lays it out, which is against the left edge.
    fn moves_lines(&self, right_to_left: bool) -> bool {
        self.align != TextAlign::Leading || right_to_left
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

/// How far each line of `layout` moves along x for `align` to place it in a
/// box `width` wide, indexed like the lines of the layout's geometry.
fn line_offsets(
    layout: &TextLayout,
    width: f32,
    align: TextAlign,
    right_to_left: bool,
) -> Vec<f32> {
    let lines = layout
        .geometry
        .as_deref()
        .map(|g| g.lines.as_slice())
        .unwrap_or(&[]);
    // A single line is as wide as the layout, which counts a trailing
    // ellipsis its geometry's box may leave out.
    if lines.len() <= 1 {
        let x = lines.first().map_or(0.0, |line| line.rect[0]);
        return vec![align.offset(width, layout.width, right_to_left) - x];
    }
    lines
        .iter()
        .map(|line| align.offset(width, line.rect[2], right_to_left) - line.rect[0])
        .collect()
}

/// `geometry` with each line moved along x by its entry in `offsets`, so the
/// text runs describe the lines where they are drawn.
fn shift_lines(geometry: &TextGeometry, offsets: &[f32]) -> TextGeometry {
    let mut shifted = geometry.clone();
    for (line, dx) in shifted.lines.iter_mut().zip(offsets) {
        line.rect[0] += dx;
        line.caret_x += dx;
        for segment in &mut line.segments {
            segment.rect[0] += dx;
        }
        if let Some(truncation) = &mut line.truncation {
            truncation.ellipsis_x += dx;
        }
    }
    shifted
}

/// Move a markup layout's spans with their lines, so a tap finds a link
/// where it is drawn.
fn shift_spans(layout: &mut TextLayout, offsets: &[f32]) {
    for span in &mut layout.spans {
        span.rect[0] += offsets.get(span.line_index).copied().unwrap_or(0.0);
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
                    WidgetEvent::PointerMove { position, .. } => {
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
                    WidgetEvent::PointerLeave { .. } => {
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
        let language = ctx.content_language().map(|tag| tag.as_str());
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

        // Markup path: the backend parses the source internally and
        // returns a TextLayout whose `spans` field carries per-run rects
        // (including links). `place_children` and `paint` stash the final
        // one for hit-testing during event dispatch.
        if self.markup {
            // Branch on the overflow mode, exactly as `paint` does. Keying
            // on whether a width was proposed instead meant an `Ellipsis`
            // markup label measured as a wrapped paragraph and painted as
            // one clipped line, so it reserved height for lines that were
            // never drawn.
            let layout = match self.overflow {
                TextOverflow::Wrap => match max_width {
                    Some(w) => backend.layout_in_language(
                        TextLayoutRequest::ParagraphMarkup {
                            source: &text,
                            max_width: w,
                            max_lines: self.max_lines,
                        },
                        &style,
                        language,
                    ),
                    None => backend.layout_in_language(
                        TextLayoutRequest::SingleLineMarkup {
                            source: &text,
                            max_width: None,
                        },
                        &style,
                        language,
                    ),
                },
                _ => backend.layout_in_language(
                    TextLayoutRequest::SingleLineMarkup {
                        source: &text,
                        max_width,
                    },
                    &style,
                    language,
                ),
            };
            return Size::new(layout.width, layout.height).into();
        }

        let is_ellipsis = matches!(self.overflow, TextOverflow::Ellipsis(_));

        let size = match self.overflow {
            TextOverflow::Wrap => match max_width {
                Some(w) => {
                    let layout = backend.layout_in_language(
                        TextLayoutRequest::Paragraph {
                            text: &text,
                            max_width: w,
                            max_lines: self.max_lines,
                        },
                        &style,
                        language,
                    );
                    Size::new(layout.width, layout.height)
                }
                None => {
                    // Unconstrained width: no basis for wrapping, so measure
                    // as a single line.
                    let layout = backend.layout_in_language(
                        TextLayoutRequest::SingleLine {
                            text: &text,
                            max_width: None,
                        },
                        &style,
                        language,
                    );
                    Size::new(layout.width, layout.height)
                }
            },
            TextOverflow::Ellipsis(EllipsisMode::Trailing) => {
                // text-typeset truncates with trailing "…" when a max_width
                // is supplied — let it do the work.
                let layout = backend.layout_in_language(
                    TextLayoutRequest::SingleLine {
                        text: &text,
                        max_width,
                    },
                    &style,
                    language,
                );
                Size::new(layout.width, layout.height)
            }
            TextOverflow::Ellipsis(mode) => match max_width {
                // Middle / Leading: compute the truncated display string
                // first, then measure it unconstrained.
                None => {
                    let layout = backend.layout_in_language(
                        TextLayoutRequest::SingleLine {
                            text: &text,
                            max_width: None,
                        },
                        &style,
                        language,
                    );
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
                    let layout = backend.layout_in_language(
                        TextLayoutRequest::SingleLine {
                            text: &truncated,
                            max_width: None,
                        },
                        &style,
                        language,
                    );
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
                .unwrap_or_else(|| {
                    backend
                        .layout_in_language(
                            TextLayoutRequest::SingleLine {
                                text: "…",
                                max_width: None,
                            },
                            &style,
                            language,
                        )
                        .width
                })
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
        let language = ctx.content_language().map(|tag| tag.as_str());
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
                TextOverflow::Wrap => backend.layout_in_language(
                    TextLayoutRequest::ParagraphMarkup {
                        source: &text,
                        max_width: max_width.max(0.0),
                        max_lines: self.max_lines,
                    },
                    &style,
                    language,
                ),
                _ => backend.layout_in_language(
                    TextLayoutRequest::SingleLineMarkup {
                        source: &text,
                        max_width: Some(max_width),
                    },
                    &style,
                    language,
                ),
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
                    let layout = backend.layout_in_language(
                        TextLayoutRequest::Paragraph {
                            text: &text,
                            max_width: max_width.max(0.0),
                            max_lines: self.max_lines,
                        },
                        &style,
                        language,
                    );
                    (Some(layout), text.clone(), None)
                }
                TextOverflow::Ellipsis(EllipsisMode::Trailing) => {
                    let layout = backend.layout_in_language(
                        TextLayoutRequest::SingleLine {
                            text: &text,
                            max_width: Some(max_width),
                        },
                        &style,
                        language,
                    );
                    (Some(layout), text.clone(), None)
                }
                TextOverflow::Ellipsis(mode) => {
                    // Middle and leading ellipsis paint a different string
                    // from the source, so the geometry has to be mapped
                    // back onto it before anyone can review the label.
                    let cut = ellipsize_ranges(&text, &style, bounds.width, mode, &mut *backend);
                    let layout = backend.layout_in_language(
                        TextLayoutRequest::SingleLine {
                            text: &cut.display,
                            max_width: None,
                        },
                        &style,
                        language,
                    );
                    let remapped = layout
                        .geometry
                        .as_deref()
                        .map(|g| Rc::new(remap_ellipsized(&text, g, &cut)));
                    (Some(layout), text.clone(), remapped)
                }
            }
        };

        let Some(mut layout) = layout else { return };
        let mut geometry = remapped.or_else(|| layout.geometry.clone());
        let right_to_left = base_direction == teksilo_core::accesskit::TextDirection::RightToLeft;
        if self.moves_lines(right_to_left) {
            let offsets = line_offsets(&layout, bounds.width, self.align, right_to_left);
            geometry = geometry.map(|g| Rc::new(shift_lines(&g, &offsets)));
            shift_spans(&mut layout, &offsets);
        }
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
        let language = ctx.content_language.as_ref().map(|tag| tag.as_str());
        let color = self.color.resolve(ctx.theme, ctx.effective_enabled);
        let style = self.style.resolve(&ctx.theme.typography);
        let right_to_left =
            ctx.layout_direction == teksilo_core::environment::LayoutDirection::RightToLeft;

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
                    TextOverflow::Wrap => backend.layout_in_language(
                        TextLayoutRequest::ParagraphMarkup {
                            source: &text,
                            max_width: (bounds.width + 0.5).max(0.0),
                            max_lines: self.max_lines,
                        },
                        &style,
                        language,
                    ),
                    _ => backend.layout_in_language(
                        TextLayoutRequest::SingleLineMarkup {
                            source: &text,
                            max_width: Some(bounds.width + 0.5),
                        },
                        &style,
                        language,
                    ),
                }
            };
            let link_color = ctx.theme.colors.text_link;
            let mut layout = layout;
            if self.moves_lines(right_to_left) {
                let offsets = line_offsets(&layout, bounds.width, self.align, right_to_left);
                canvas.draw_text_layout_with_line_offsets(
                    &layout,
                    Point::new(bounds.x, bounds.y),
                    color,
                    link_color,
                    &offsets,
                );
                shift_spans(&mut layout, &offsets);
            } else {
                canvas.draw_text_layout_markup(
                    &layout,
                    teksilo_canvas::Point::new(bounds.x, bounds.y),
                    color,
                    link_color,
                );
            }
            // Keep the cached layout in sync so tap hit-testing sees
            // the same rects that were painted.
            *self.last_layout.borrow_mut() = Some(layout);
            return;
        }

        // Lines placed by alignment: lay the text out as the plain draws
        // below do, then draw each line where it goes.
        if self.moves_lines(right_to_left)
            && let Some(backend_rc) = canvas.text_backend().cloned()
        {
            let max_width = bounds.width + 0.5;
            let layout = {
                let mut backend = backend_rc.borrow_mut();
                match self.overflow {
                    TextOverflow::Wrap => backend.layout_in_language(
                        TextLayoutRequest::Paragraph {
                            text: &text,
                            max_width: max_width.max(0.0),
                            max_lines: self.max_lines,
                        },
                        &style,
                        language,
                    ),
                    TextOverflow::Ellipsis(EllipsisMode::Trailing) => backend.layout_in_language(
                        TextLayoutRequest::SingleLine {
                            text: &text,
                            max_width: Some(max_width),
                        },
                        &style,
                        language,
                    ),
                    TextOverflow::Ellipsis(mode) => {
                        let truncated = teksilo_canvas::ellipsis::ellipsize(
                            &text,
                            &style,
                            bounds.width,
                            mode,
                            &mut *backend,
                        );
                        backend.layout_in_language(
                            TextLayoutRequest::SingleLine {
                                text: &truncated,
                                max_width: Some(max_width),
                            },
                            &style,
                            language,
                        )
                    }
                }
            };
            let offsets = line_offsets(&layout, bounds.width, self.align, right_to_left);
            canvas.draw_text_layout_with_line_offsets(
                &layout,
                Point::new(bounds.x, bounds.y),
                color,
                color,
                &offsets,
            );
            return;
        }

        match self.overflow {
            TextOverflow::Wrap => {
                canvas.draw_paragraph_in_language(
                    &text,
                    bounds,
                    &style,
                    color,
                    self.max_lines,
                    language,
                );
            }
            TextOverflow::Ellipsis(EllipsisMode::Trailing) => {
                canvas.draw_text_in_language(&text, bounds, &style, color, language);
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
                canvas.draw_text_in_language(&truncated, bounds, &style, color, language);
            }
        }
    }

    /// Exposed so a test can read what a label draws. A label hidden from
    /// assistive technology publishes no node, so there is nowhere else to
    /// read its text from.
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
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

    /// A mock backend that records the language of every layout it is asked
    /// for, and the text.
    struct LanguageRecorder {
        inner: MockTextBackend,
        asked: Rc<RefCell<Vec<(String, Option<String>)>>>,
    }

    impl teksilo_canvas::TextBackend for LanguageRecorder {
        fn layout_single_line(
            &mut self,
            text: &str,
            style: &teksilo_tokens::TextStyle,
            max_width: Option<f32>,
        ) -> TextLayout {
            self.asked.borrow_mut().push((text.to_owned(), None));
            self.inner.layout_single_line(text, style, max_width)
        }

        fn layout_in_language(
            &mut self,
            request: TextLayoutRequest<'_>,
            style: &teksilo_tokens::TextStyle,
            language: Option<&str>,
        ) -> TextLayout {
            let text = match request {
                TextLayoutRequest::SingleLine { text, .. }
                | TextLayoutRequest::Paragraph { text, .. } => text,
                TextLayoutRequest::SingleLineMarkup { source, .. }
                | TextLayoutRequest::ParagraphMarkup { source, .. } => source,
            };
            self.asked
                .borrow_mut()
                .push((text.to_owned(), language.map(str::to_owned)));
            teksilo_canvas::lay_out_ignoring_language(&mut self.inner, request, style)
        }

        fn ensure_glyphs(&mut self, layout: &TextLayout) -> Vec<teksilo_canvas::GlyphQuad> {
            self.inner.ensure_glyphs(layout)
        }
    }

    /// Every layout a label asks for, in layout and in paint, wrapping or
    /// not, is asked in the language declared around it.
    #[test]
    fn a_label_lays_out_and_paints_in_its_content_language() {
        use teksilo_core::widget_builder::WidgetBuilder;
        let asked = Rc::new(RefCell::new(Vec::new()));
        let backend = LanguageRecorder {
            inner: MockTextBackend::new(),
            asked: asked.clone(),
        };
        let mut tree = WidgetTree::new().with_text_backend(Rc::new(RefCell::new(backend)));
        let japanese = tree.add(TextWidget::new(lit!("日本語")));
        let wrapped = tree.add(TextWidget::new(lit!("中文 段落")).overflow(TextOverflow::Wrap));
        let plain = tree.add(TextWidget::new(lit!("français")));
        let column = tree.add(
            crate::primitives::VStack::new()
                .child(japanese)
                .child(wrapped)
                .content_language(teksilo_core::language::LanguageTag::parse("ja").unwrap()),
        );
        tree.add(crate::primitives::VStack::new().child(column).child(plain));
        tree.layout(SizeProposal::exact(200.0, 200.0));
        tree.render();

        let asked = asked.borrow();
        let languages_of = |text: &str| -> Vec<Option<String>> {
            asked
                .iter()
                .filter(|(t, _)| t == text)
                .map(|(_, l)| l.clone())
                .collect()
        };
        for text in ["日本語", "中文 段落"] {
            let seen = languages_of(text);
            assert!(!seen.is_empty(), "{text} was laid out");
            assert!(
                seen.iter().all(|l| l.as_deref() == Some("ja")),
                "{text}: {seen:?}"
            );
        }
        let seen = languages_of("français");
        assert!(
            !seen.is_empty() && seen.iter().all(Option::is_none),
            "{seen:?}"
        );
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
        let _stack = tree.add(HStack::new().child(label));
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
        let _stack = tree.add(HStack::new().child(label));
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
        let _stack = tree.add(HStack::new().child(label));
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

    // ── Line alignment ──────────────────────────────────────────────

    /// Wraps at 200 dp into "A longer sentence that " (23 characters,
    /// 184 dp with its trailing space) and "wraps on two lines" (18
    /// characters, 144 dp).
    const SENTENCE: &str = "A longer sentence that wraps on two lines";

    /// The left and right edges of each line of glyphs drawn inside the
    /// bounds of `id`, top to bottom.
    fn drawn_lines(tree: &mut WidgetTree, id: WidgetId) -> Vec<(f32, f32)> {
        let area = tree.bounds(id);
        let frame = tree.render();
        let mut lines: Vec<(f32, f32, f32)> = Vec::new();
        for glyph in &frame.glyphs {
            let [x, y, w, h] = glyph.screen;
            if !area.contains(Point::new(x + w * 0.5, y + h * 0.5)) {
                continue;
            }
            match lines.iter_mut().find(|line| line.0 == y) {
                Some(line) => {
                    line.1 = line.1.min(x);
                    line.2 = line.2.max(x + w);
                }
                None => lines.push((y, x, x + w)),
            }
        }
        lines.sort_by(|a, b| a.0.total_cmp(&b.0));
        lines
            .into_iter()
            .map(|(_, left, right)| (left, right))
            .collect()
    }

    /// `text` in a box 200 dp wide.
    fn in_a_200_box(tree: &mut WidgetTree, text: TextWidget) -> WidgetId {
        let id = tree.add(text);
        tree.add(crate::primitives::FixedSize::new().width(200.0).child(id));
        tree.layout(SizeProposal::exact(200.0, 300.0));
        assert_eq!(tree.bounds(id).width, 200.0, "precondition: the box");
        id
    }

    fn assert_close(actual: &[(f32, f32)], expected: &[(f32, f32)]) {
        assert_eq!(actual.len(), expected.len(), "{actual:?}");
        for (a, e) in actual.iter().zip(expected) {
            assert!(
                (a.0 - e.0).abs() < 0.01 && (a.1 - e.1).abs() < 0.01,
                "{actual:?} != {expected:?}"
            );
        }
    }

    use teksilo_core::widget_id::WidgetId;

    /// The issue's reproduction: a title and a wrapped sentence centred in
    /// a 200 dp column. `Center` and the stack's alignment centre the
    /// block; `text_align` centres each line of it, so both lines of the
    /// sentence are centred on the column, not only the longer one.
    #[test]
    fn each_line_of_a_centred_wrapped_text_is_centred() {
        use crate::primitives::{Center, VStack};
        let mut tree = tree_with_mock_backend();
        let sentence = tree.add(TextWidget::new(lit!(SENTENCE)).text_align(TextAlign::Center));
        tree.add(
            Center::new().child(
                VStack::new()
                    .alignment(teksilo_tokens::HAlignment::Center)
                    .child(TextWidget::new(lit!("A short title")))
                    .child(sentence),
            ),
        );
        tree.layout(SizeProposal::exact(200.0, 300.0));
        assert_eq!(
            tree.bounds(sentence).width,
            184.0,
            "the text still reports its widest line"
        );
        let lines = drawn_lines(&mut tree, sentence);
        assert_close(&lines, &[(8.0, 192.0), (28.0, 172.0)]);
    }

    /// Lines are placed within the width given, wider than the text.
    #[test]
    fn trailing_lines_end_at_the_boxes_right_edge() {
        let mut tree = tree_with_mock_backend();
        let id = in_a_200_box(
            &mut tree,
            TextWidget::new(lit!(SENTENCE)).text_align(TextAlign::Trailing),
        );
        let lines = drawn_lines(&mut tree, id);
        assert_close(&lines, &[(16.0, 200.0), (56.0, 200.0)]);
    }

    /// The default is unchanged in a left-to-right layout, and puts the
    /// lines against the right edge in a right-to-left one.
    #[test]
    fn leading_lines_follow_the_layout_direction() {
        let mut tree = tree_with_mock_backend();
        let id = in_a_200_box(&mut tree, TextWidget::new(lit!(SENTENCE)));
        let lines = drawn_lines(&mut tree, id);
        assert_close(&lines, &[(0.0, 184.0), (0.0, 144.0)]);

        let mut tree = tree_with_mock_backend();
        tree.set_layout_direction(teksilo_core::environment::LayoutDirection::RightToLeft);
        let id = in_a_200_box(&mut tree, TextWidget::new(lit!(SENTENCE)));
        let lines = drawn_lines(&mut tree, id);
        assert_close(&lines, &[(16.0, 200.0), (56.0, 200.0)]);
    }

    /// A single line narrower than its box is placed the same way, the
    /// elided kind too.
    #[test]
    fn a_single_line_is_placed_in_its_box() {
        let mut tree = tree_with_mock_backend();
        let id = in_a_200_box(
            &mut tree,
            TextWidget::new(lit!("Hello"))
                .single_line()
                .text_align(TextAlign::Center),
        );
        let lines = drawn_lines(&mut tree, id);
        assert_close(&lines, &[(80.0, 120.0)]);
    }

    /// The text runs a reader reviews are where the lines are drawn.
    #[test]
    fn text_runs_follow_the_aligned_lines() {
        use teksilo_core::accesskit::Role;
        let mut tree = tree_with_mock_backend();
        let id = in_a_200_box(
            &mut tree,
            TextWidget::new(lit!(SENTENCE)).text_align(TextAlign::Center),
        );
        let lines = drawn_lines(&mut tree, id);
        assert_close(&lines, &[(8.0, 192.0), (28.0, 172.0)]);
        let update = tree.sync_accessibility();
        let mut runs: Vec<(f64, f64)> = update
            .nodes
            .iter()
            .filter(|(_, node)| node.role() == Role::TextRun)
            .filter_map(|(_, node)| node.bounds().map(|b| (b.x0, b.x1)))
            .collect();
        runs.sort_by(|a, b| a.0.total_cmp(&b.0));
        let runs: Vec<(f32, f32)> = runs
            .into_iter()
            .map(|(x0, x1)| (x0 as f32, x1 as f32))
            .collect();
        assert_close(&runs, &[(8.0, 192.0), (28.0, 172.0)]);
    }

    /// The mock, with a span for each link of a single-line markup layout,
    /// which the mock leaves out and a tap on a link needs.
    struct LinkSpans(MockTextBackend);

    impl teksilo_canvas::TextBackend for LinkSpans {
        fn layout_single_line(
            &mut self,
            text: &str,
            style: &teksilo_tokens::TextStyle,
            max_width: Option<f32>,
        ) -> TextLayout {
            self.0.layout_single_line(text, style, max_width)
        }

        fn layout_single_line_markup(
            &mut self,
            source: &str,
            style: &teksilo_tokens::TextStyle,
            max_width: Option<f32>,
        ) -> TextLayout {
            let mut layout = self.0.layout_single_line_markup(source, style, max_width);
            let geometry = layout.geometry.clone().expect("the mock reports geometry");
            let rendered = geometry.rendered_text.as_deref().unwrap_or(source);
            for link in &geometry.links {
                let range = link.rendered_byte_range.clone();
                let start = rendered[..range.start].chars().count() as f32 * 8.0;
                let width = rendered[range.clone()].chars().count() as f32 * 8.0;
                layout.spans.push(teksilo_canvas::TextLayoutSpan {
                    kind: teksilo_canvas::TextSpanKind::Link {
                        url: link.url.clone(),
                    },
                    line_index: 0,
                    rect: [start, 0.0, width, 16.0],
                    byte_range: range,
                });
            }
            layout
        }

        fn ensure_glyphs(&mut self, layout: &TextLayout) -> Vec<teksilo_canvas::GlyphQuad> {
            self.0.ensure_glyphs(layout)
        }
    }

    /// A tap finds a link where its line was moved to, and the link is
    /// drawn in the link colour there.
    #[test]
    fn a_link_is_tapped_where_its_line_is_drawn() {
        let tapped = Rc::new(RefCell::new(Vec::<String>::new()));
        let mut tree = WidgetTree::new()
            .with_text_backend(Rc::new(RefCell::new(LinkSpans(MockTextBackend::new()))));
        let id = in_a_200_box(
            &mut tree,
            TextWidget::new(lit!("[Help](help)"))
                .single_line()
                .text_align(TextAlign::Center)
                .on_link_click({
                    let tapped = tapped.clone();
                    move |url, _| tapped.borrow_mut().push(url.to_string())
                }),
        );
        let lines = drawn_lines(&mut tree, id);
        assert_close(&lines, &[(84.0, 116.0)]);
        let link = tree.theme().colors.text_link.to_array();
        assert!(
            tree.render().glyphs.iter().all(|g| g.color == link),
            "drawn in the link colour"
        );
        tree.tap_with(teksilo_tokens::PointerKind::Mouse, Point::new(10.0, 8.0));
        assert!(
            tapped.borrow().is_empty(),
            "nothing is drawn at the left edge"
        );
        tree.tap_with(teksilo_tokens::PointerKind::Mouse, Point::new(100.0, 8.0));
        assert_eq!(*tapped.borrow(), ["help"]);
    }
}
