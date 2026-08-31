// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! ScrollArea — a clipping viewport that scrolls its content on wheel, touch,
//! and assistive-technology actions.
//!
//! Wrap any widget in `ScrollArea` to make it scrollable. The scroll position
//! is stored in reactive `Signal<f32>` signals (one per axis), shared with the
//! built-in [`ScrollBar`] children. Two display
//! modes cover most use cases: `Overlay` (the default, macOS-style thin-at-rest
//! indicator that expands on hover) and `Permanent` (a layout-consuming gutter
//! always on screen). Use [`ScrollBarPolicy`] to control when each axis shows.
//!
//! ## Accessibility
//!
//! Reports `Role::ScrollView` with per-axis `scroll_y` / `scroll_x` position
//! and limit fields. Advertises `ScrollUp` / `ScrollDown` / `ScrollLeft` /
//! `ScrollRight` actions only for the axes that actually overflow, so AT clients
//! (NVDA, JAWS, VoiceOver) know which directions are reachable.
//!
//! ```rust
//! # use teksilo_widgets::scroll_area::{ScrollArea, ScrollBarMode};
//! # use teksilo_widgets::primitives::MinSize;
//! let _w = ScrollArea::new()
//!     .child(MinSize::new(0.0, 2000.0))
//!     .scroll_bar_style(ScrollBarMode::Permanent)
//!     .smooth_scrolling(true);
//! ```

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use teksilo_canvas::{Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::ColorProp;
use teksilo_core::event::{EventResponse, ScrollDelta, WidgetEvent};
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::widget::{LayoutContext, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::Easing;

use crate::common::scroll::OverscrollBehavior;
use crate::scroll_bar::{ScrollBar, ScrollBarOrientation, ScrollBarVisual};

/// How the scroll bar is presented relative to the viewport content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollBarMode {
    /// Scroll bar overlays the content (macOS-style): a thin passive indicator
    /// is painted while scrolling; the full interactive track expands on pointer
    /// proximity. Does not reduce the viewport width.
    #[default]
    Overlay,
    /// Scroll bar is a permanent layout sibling of the viewport, reserving its
    /// full thickness and always remaining interactive — the classic Windows/Linux
    /// gutter style.
    Permanent,
    /// Floats over the content like `Overlay` but only ever shows the thin resting
    /// indicator, never the full track. A passive scroll-position display for
    /// minimal UIs; drag, track-click, and keyboard still work against the full
    /// slot bounds.
    Thin,
}

/// Controls when the scroll bar appears for a given axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollBarPolicy {
    /// Show the scroll bar only when content exceeds the viewport size (default).
    #[default]
    AsNeeded,
    /// Always show the scroll bar, even when content fits without scrolling.
    AlwaysOn,
    /// Never show the scroll bar; content is still scrollable via wheel and touch.
    AlwaysOff,
}

/// A clipping viewport that makes any child widget scrollable.
///
/// The scroll offset per axis is stored in a reactive `Signal<f32>`, shared
/// with the built-in `ScrollBar` children. See [`ScrollBarMode`] for display
/// options and [`ScrollBarPolicy`] for per-axis visibility control.
pub struct ScrollArea {
    content_child: Option<Box<dyn Widget>>,
    content_child_id: Option<WidgetId>,
    scroll_bar_style: ScrollBarMode,
    /// Per-axis scroll bar visibility policy.
    vertical_policy: ScrollBarPolicy,
    horizontal_policy: ScrollBarPolicy,
    /// Pixels per scroll line (for line-based mouse wheel events).
    line_height: f32,
    /// Thickness of the scroll bar (for permanent mode layout).
    scroll_bar_thickness: f32,
    /// Optional thumb tint forwarded to the built-in scroll bars. `None`
    /// (default) paints from the theme's `scrollbar_thumb*` tokens. See
    /// [`Self::scroll_bar_thumb_color`].
    scroll_bar_thumb_color: Option<ColorProp>,
    /// When true, content smaller than the viewport is stretched to fill it.
    widget_resizable: bool,
    /// Whether line-based scroll events animate smoothly to their target.
    smooth_scrolling: bool,
    /// Duration of the smooth scroll animation.
    smooth_scroll_duration: Duration,
    /// Preferred size returned by `size_that_fits` when the proposal is
    /// unconstrained. `None` falls back to cached content size or 300×200.
    preferred_size: Option<Size>,
    /// Height-only cap; width still follows the content. See `preferred_height`.
    preferred_height: Option<f32>,
    /// Scroll-chaining behavior at the boundary. `Chain` (default) lets a
    /// boundary scroll bubble to an ancestor scrollable; `Contain` absorbs it
    /// (the web's `overscroll-behavior`).
    overscroll_behavior: OverscrollBehavior,
    /// Extra scrollable range past the end of the content, as a fraction of the
    /// viewport height. See [`Self::scroll_past_end`].
    scroll_past_end: Prop<f32>,

    // --- shared reactive state ---
    /// Vertical scroll position (0.0 = top).
    scroll_y: Signal<f32>,
    /// Horizontal scroll position (0.0 = left).
    scroll_x: Signal<f32>,
    /// Maximum vertical scroll (content_height - viewport_height).
    max_scroll_y: Signal<f32>,
    /// Maximum horizontal scroll (content_width - viewport_width).
    max_scroll_x: Signal<f32>,
    /// Vertical viewport/content ratio (0.0..1.0).
    viewport_ratio_y: Signal<f32>,
    /// Horizontal viewport/content ratio (0.0..1.0).
    viewport_ratio_x: Signal<f32>,

    // --- resolved children ---
    /// Resolved child IDs: [content, optional_v_scrollbar, optional_h_scrollbar]
    child_ids: Vec<WidgetId>,

    // --- cached sizes for event handling ---
    content_size: Cell<Size>,
    /// Shared with the on_scroll / on_access_action handler closures.
    /// Wrapped in `Rc` because cloning a bare `Cell` produces an
    /// independent cell — the closure would never see updates from
    /// `place_children`.
    viewport_size: Rc<Cell<Size>>,
    /// Absolute top-left of the viewport in tree/screen coordinates.
    /// Needed to convert `target_bounds` (which `ScrollIntoView` carries
    /// in absolute tree coords) into content-relative coordinates.
    /// Shared via `Rc` for the same reason as `viewport_size`.
    viewport_origin: Rc<Cell<Point>>,

    // --- one-shot restore ---
    /// A vertical offset waiting for a range long enough to hold it. See
    /// [`Self::restore_scroll_y`]. Shared via `Rc` because the scroll handler
    /// stands it down when the reader takes over, and that closure cannot borrow
    /// `self`.
    pending_restore_y: Rc<Cell<Option<f32>>>,
    /// What the pending restore last wrote to `scroll_y`, so a write by anyone
    /// else can be recognised on the following pass.
    ///
    /// The `on_scroll` handler stands the restore down for a wheel gesture and a
    /// `ScrollIntoView`, which is every route that reaches *it* — but not every
    /// route that moves the scroll. **A scroll bar holds a clone of `scroll_y`
    /// and calls `set` on it directly** (`ScrollBar::new` is handed the signal in
    /// `build`), so dragging the thumb never reaches that handler. With a pending
    /// offset the content is too short to ever honour, the drag was undone by the
    /// next layout pass and the reader was pinned at the clamped bottom with no
    /// way out.
    ///
    /// Shared via `Rc` for the same reason as `pending_restore_y`: it is cleared
    /// beside it, from a closure that cannot borrow `self`.
    restore_wrote_y: Rc<Cell<Option<f32>>>,
}

impl Default for ScrollArea {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ScrollArea {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollArea")
            .field("scroll_y", &self.scroll_y.get())
            .field("scroll_x", &self.scroll_x.get())
            .field("style", &self.scroll_bar_style)
            .field("v_policy", &self.vertical_policy)
            .field("h_policy", &self.horizontal_policy)
            .field("widget_resizable", &self.widget_resizable)
            .field("content_size", &self.content_size.get())
            .field("viewport_size", &self.viewport_size.get())
            .finish()
    }
}

impl ScrollArea {
    /// Create a new `ScrollArea` with overlay scroll bars, smooth scrolling, and no content yet.
    pub fn new() -> Self {
        Self {
            content_child: None,
            content_child_id: None,
            scroll_bar_style: ScrollBarMode::default(),
            vertical_policy: ScrollBarPolicy::default(),
            horizontal_policy: ScrollBarPolicy::default(),
            line_height: 20.0,
            scroll_bar_thickness: 12.0,
            scroll_bar_thumb_color: None,
            widget_resizable: false,
            smooth_scrolling: true,
            smooth_scroll_duration: Duration::from_millis(150),
            preferred_size: None,
            preferred_height: None,
            overscroll_behavior: OverscrollBehavior::default(),
            scroll_past_end: Prop::Static(0.0),
            scroll_y: Signal::new_animated(0.0),
            scroll_x: Signal::new_animated(0.0),
            max_scroll_y: Signal::new(0.0),
            max_scroll_x: Signal::new(0.0),
            viewport_ratio_y: Signal::new(1.0),
            viewport_ratio_x: Signal::new(1.0),
            child_ids: Vec::new(),
            content_size: Cell::new(Size::ZERO),
            viewport_size: Rc::new(Cell::new(Size::ZERO)),
            viewport_origin: Rc::new(Cell::new(Point::ZERO)),
            pending_restore_y: Rc::new(Cell::new(None)),
            restore_wrote_y: Rc::new(Cell::new(None)),
        }
    }

    /// Set the scrollable content widget.
    pub fn child(mut self, child: impl Widget + 'static) -> Self {
        self.content_child = Some(Box::new(child));
        self.content_child_id = None;
        self
    }

    /// Construct from an already-registered child WidgetId.
    pub fn from_id(child: WidgetId) -> Self {
        let mut sa = Self::new();
        sa.content_child_id = Some(child);
        sa
    }

    /// Set the scroll bar display mode (`Overlay`, `Permanent`, or `Thin`).
    pub fn scroll_bar_style(mut self, style: ScrollBarMode) -> Self {
        self.scroll_bar_style = style;
        self
    }

    /// Tint the built-in scroll bars' thumb with an explicit colour instead of
    /// the theme's `scrollbar_thumb*` tokens. Accepts anything
    /// `impl Into<ColorProp>` — a `Color`, a theme role, or a `Signal` —
    /// resolved against the live theme at paint, so roles/signals stay
    /// reactive. Forwarded to both scroll bars via
    /// [`ScrollBar::thumb_color`](crate::scroll_bar::ScrollBar::thumb_color).
    /// Use when the area sits on a surface the surface-relative tokens don't
    /// suit — e.g. a tooltip's inverse chip (`TextRole::TooltipText`).
    pub fn scroll_bar_thumb_color(mut self, color: impl Into<ColorProp>) -> Self {
        self.scroll_bar_thumb_color = Some(color.into());
        self
    }

    /// Set the vertical scroll bar visibility policy.
    pub fn vertical_scroll_bar_policy(mut self, policy: ScrollBarPolicy) -> Self {
        self.vertical_policy = policy;
        self
    }

    /// Set the horizontal scroll bar visibility policy.
    pub fn horizontal_scroll_bar_policy(mut self, policy: ScrollBarPolicy) -> Self {
        self.horizontal_policy = policy;
        self
    }

    /// Set the pixels-per-line used when translating line-based wheel events.
    pub fn line_height(mut self, lh: f32) -> Self {
        self.line_height = lh;
        self
    }

    /// Set the scroll bar thickness in logical pixels (applies to both axes).
    pub fn scroll_bar_thickness(mut self, thickness: f32) -> Self {
        self.scroll_bar_thickness = thickness;
        self
    }

    /// When true, content smaller than the viewport is stretched to fill it.
    /// Similar to Qt's `QScrollArea::setWidgetResizable(true)`.
    pub fn widget_resizable(mut self, resizable: bool) -> Self {
        self.widget_resizable = resizable;
        self
    }

    /// Enable or disable smooth animated scrolling for wheel events.
    /// Enabled by default. Applies to both line-based (`ScrollDelta::Lines`)
    /// and pixel-based (`ScrollDelta::Pixels`) wheel events — on Wayland and
    /// other platforms with high-resolution scroll axes, mouse wheel notches
    /// are delivered as pixel deltas, so animating both paths is required for
    /// a fast flick to feel smooth instead of jumping.
    pub fn smooth_scrolling(mut self, enabled: bool) -> Self {
        self.smooth_scrolling = enabled;
        self
    }

    /// Set the duration of the smooth scroll animation (default: 150ms).
    pub fn smooth_scroll_duration(mut self, duration: Duration) -> Self {
        self.smooth_scroll_duration = duration;
        self
    }

    /// Allow scrolling past the end of the content by `fraction` of the
    /// viewport height (default `0.0` — the last pixel of content stops flush
    /// with the bottom of the viewport).
    ///
    /// This extends the scroll **range** only. It adds no widget, no padding and
    /// no layout, so it cannot interfere with the content's own padding — a
    /// distinction worth keeping, since padding-based implementations of this
    /// idea in other toolkits are a recurring source of "single-line content is
    /// scrollable" bugs.
    ///
    /// The motivating case is typewriter scrolling: to pin the caret's line at
    /// the middle of the viewport, the view must be able to scroll half a
    /// viewport past the last line, or the pin quietly stops working over the
    /// final page — exactly where a writer spends their time. Pair with
    /// [`EventContext::ensure_visible_aligned`], passing `1.0 - fraction` here
    /// for a pin at `fraction`.
    ///
    /// Accepts a literal or a `Signal<f32>`, so it can follow a setting live.
    /// Negative values are treated as `0.0`.
    ///
    /// [`EventContext::ensure_visible_aligned`]: teksilo_core::widget::EventContext::ensure_visible_aligned
    pub fn scroll_past_end(mut self, fraction: impl Into<Prop<f32>>) -> Self {
        self.scroll_past_end = fraction.into();
        self
    }

    /// Set a preferred size returned when the parent proposes unconstrained
    /// dimensions. If not set, falls back to cached content size or 300×200.
    ///
    /// This overrides **both** axes. If you only want to cap the height and let
    /// the width follow the content — the usual case for a menu or popover, which
    /// must be as wide as its widest row — use [`preferred_height`] instead.
    /// Passing a width of `0.0` here does *not* mean "no preference": it means
    /// zero, and the scroll area will collapse.
    ///
    /// [`preferred_height`]: Self::preferred_height
    pub fn preferred_size(mut self, width: f32, height: f32) -> Self {
        self.preferred_size = Some(Size::new(width, height));
        self
    }

    /// The content's natural width, for reporting an intrinsic width to a parent
    /// that hugs (a menu, a popover).
    ///
    /// **Measured, not remembered.** `content_size` is only populated in
    /// `place_children`, so on the very first layout pass — which is exactly when
    /// a popover decides how wide to be — it is still zero, and the old code fell
    /// back to a hard-coded `300.0`. That is how a menu of long rows ended up
    /// narrower than its own content and clipped every one of them. Measuring the
    /// child with an unbounded width asks it what it actually wants.
    ///
    /// Falls back to the cached size, then to `300.0`, if the child cannot be
    /// measured (no content child yet).
    fn natural_content_width(&self, ctx: &LayoutContext) -> f32 {
        // The content child is `child_ids[0]` — `content_child` / `content_child_id`
        // are both *consumed* by `build()`, so they are `None` by layout time.
        if let Some(&child) = self.child_ids.first()
            && let Some(size) = ctx.child_size(
                child,
                SizeProposal {
                    width: None,
                    height: None,
                },
            )
            && size.width > 0.0
        {
            return size.width;
        }
        let cached = self.content_size.get().width;
        if cached > 0.0 { cached } else { 300.0 }
    }

    /// Cap the height when the parent proposes an unconstrained one, while
    /// letting the **width** continue to follow the content.
    ///
    /// This is what a scrolling menu/popover wants: it must not grow taller than
    /// its viewport, but it must still be as wide as its widest item. Using
    /// [`preferred_size`](Self::preferred_size) with a `0.0` width for this
    /// collapses the panel to its minimum width and clips every row — the parent
    /// proposes an unconstrained width (it is hugging its content), so the `0.0`
    /// is taken literally.
    pub fn preferred_height(mut self, height: f32) -> Self {
        self.preferred_height = Some(height);
        self
    }

    /// Set the scroll-chaining behavior at the boundary. Default
    /// [`OverscrollBehavior::Chain`] (a boundary scroll bubbles to an ancestor
    /// scrollable); [`OverscrollBehavior::Contain`] absorbs it instead.
    pub fn overscroll_behavior(mut self, behavior: OverscrollBehavior) -> Self {
        self.overscroll_behavior = behavior;
        self
    }

    /// Land `offset` on the first layout pass at which this area has a real
    /// scrollable range, then forget it.
    ///
    /// `max_scroll_y` is `0.0` until the content has been measured, so an
    /// offset a host writes before that first measurement is clamped away to
    /// zero and the page paints at the top for a frame before jumping to
    /// where it should have started. This stores the offset instead and
    /// applies it itself, inside layout, as soon as `max_scroll_y` becomes
    /// nonzero, before the ordinary clamp would otherwise discard it, so the
    /// very first frame the content is measured on is already laid out at
    /// the restored position, with no visible jump.
    ///
    /// It is a one-shot: once applied, it is dropped, so a later reflow (a
    /// wider window, an edit that lengthens the document) never yanks the
    /// reader back to where they came in. The offset is still clamped to the
    /// real range when it lands: past the end it lands at the end, negative
    /// it lands at zero.
    ///
    /// `offset <= 0.0` is a no-op: there is nothing to restore, and it clears
    /// any previously armed offset rather than leaving it pending.
    ///
    /// An area that never calls this behaves exactly as it always has.
    pub fn restore_scroll_y(self, offset: f32) -> Self {
        // Set through the existing cell rather than replacing it: the scroll handler
        // captured this `Rc` when the area was constructed, and handing it a fresh
        // one would leave it standing down a slot nothing reads.
        self.pending_restore_y.set((offset > 0.0).then_some(offset));
        // A fresh one-shot has written nothing yet. Left over from a previous
        // arming on the same area, this would make the first pass mistake the
        // *old* landing for somebody else's write and stand the new offset down
        // before it had a chance.
        self.restore_wrote_y.set(None);
        self
    }

    /// Get the vertical scroll position signal (for external observation).
    pub fn scroll_y_signal(&self) -> &Signal<f32> {
        &self.scroll_y
    }

    /// Get the horizontal scroll position signal (for external observation).
    pub fn scroll_x_signal(&self) -> &Signal<f32> {
        &self.scroll_x
    }

    /// Maximum vertical scroll offset for the current content
    /// (`content_height − viewport_height`, or 0 when content fits), plus any
    /// range bought with [`scroll_past_end`](Self::scroll_past_end).
    /// External callers bind to this for "is there more to scroll?"
    /// chrome (e.g. trailing scroll-arrow visibility).
    pub fn max_scroll_y_signal(&self) -> &Signal<f32> {
        &self.max_scroll_y
    }

    /// Fraction of the scrollable height currently visible (`1.0` when
    /// everything fits) — what sizes the vertical scroll bar's thumb. Accounts
    /// for [`scroll_past_end`](Self::scroll_past_end), so the thumb stays
    /// proportional to the range the user can actually travel.
    pub fn viewport_ratio_y_signal(&self) -> &Signal<f32> {
        &self.viewport_ratio_y
    }

    /// Maximum horizontal scroll offset for the current content.
    /// External callers bind to this for "is there more to scroll?"
    /// chrome (e.g. trailing scroll-arrow visibility on a tab bar).
    pub fn max_scroll_x_signal(&self) -> &Signal<f32> {
        &self.max_scroll_x
    }

    /// The viewport size this area last placed its content into, shared
    /// live (an `Rc<Cell<_>>`, not a snapshot).
    ///
    /// Deliberately not public: it reports the *previous* layout pass, so
    /// it is only sound for a widget that also knows when that pass is
    /// still current. `TabBar` reads it to resolve the axis its own
    /// measurement leaves unbounded — a vertical bar's content is
    /// measured with `height: None`, so the row cannot recover the
    /// viewport height from its size proposal.
    pub(crate) fn viewport_size_cell(&self) -> Rc<Cell<Size>> {
        self.viewport_size.clone()
    }

    fn clamp_and_set_scroll(&self) {
        let max_y = self.max_scroll_y.get();
        let max_x = self.max_scroll_x.get();
        let cur_y = self.scroll_y.get();
        let cur_x = self.scroll_x.get();
        let clamped_y = cur_y.clamp(0.0, max_y);
        let clamped_x = cur_x.clamp(0.0, max_x);
        if (clamped_y - cur_y).abs() > f32::EPSILON {
            self.scroll_y.set(clamped_y);
        }
        if (clamped_x - cur_x).abs() > f32::EPSILON {
            self.scroll_x.set(clamped_x);
        }
    }
}

impl Widget for ScrollArea {
    /// Opt into concrete-type introspection so a host's tests can read the
    /// scroll metrics of an area built deep inside a composite (a page whose
    /// `ScrollArea` no caller holds a reference to) rather than only of one they
    /// constructed themselves.
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let mut ids = Vec::new();

        // Resolve the content child
        let content_id = if let Some(child) = self.content_child.take() {
            ctx.add_boxed(child)
        } else if let Some(id) = self.content_child_id.take() {
            id
        } else if !self.child_ids.is_empty() {
            // Already built — return existing children
            return self.child_ids.clone();
        } else {
            // No content was set (e.g. `ScrollArea::default()` reaching the
            // tree). `build()` must never panic — an empty content area is a
            // valid, if useless, widget: `place_children` early-returns on an
            // empty child list and `layout_response` falls back to its default
            // size. Leave `child_ids` empty and render nothing.
            self.child_ids.clear();
            return Vec::new();
        };
        ids.push(content_id);

        // Scrollbar visual tuning depends on mode
        let visual = match self.scroll_bar_style {
            ScrollBarMode::Permanent => ScrollBarVisual::Permanent,
            ScrollBarMode::Overlay => ScrollBarVisual::Overlay,
            ScrollBarMode::Thin => ScrollBarVisual::Thin,
        };
        let thickness = self.scroll_bar_thickness; // full thickness for all modes

        // Create vertical scrollbar
        let mut v_scrollbar = ScrollBar::new(
            ScrollBarOrientation::Vertical,
            self.scroll_y.clone(),
            self.max_scroll_y.clone(),
            self.viewport_ratio_y.clone(),
        )
        .thickness(thickness)
        .visual(visual);
        if let Some(tint) = &self.scroll_bar_thumb_color {
            v_scrollbar = v_scrollbar.thumb_color(tint.clone());
        }
        let v_id = ctx.add(v_scrollbar);
        ids.push(v_id);

        // Create horizontal scrollbar
        let mut h_scrollbar = ScrollBar::new(
            ScrollBarOrientation::Horizontal,
            self.scroll_x.clone(),
            self.max_scroll_x.clone(),
            self.viewport_ratio_x.clone(),
        )
        .thickness(thickness)
        .visual(visual);
        if let Some(tint) = &self.scroll_bar_thumb_color {
            h_scrollbar = h_scrollbar.thumb_color(tint.clone());
        }
        let h_id = ctx.add(h_scrollbar);
        ids.push(h_id);

        // Register animated signals
        ctx.register_animated_signal(&self.scroll_y);
        ctx.register_animated_signal(&self.scroll_x);

        // Register bindings: scroll position changes trigger relayout (content offset moves)
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.scroll_y
            .bind_to(self_id, registry, BindingLevel::Relayout);
        self.scroll_x
            .bind_to(self_id, registry, BindingLevel::Relayout);
        // An *input* to the scroll range, unlike the metrics published at the
        // bottom of `layout` — binding it at `Relayout` is safe (nothing writes
        // it during layout) and is what makes a live settings change re-measure.
        self.scroll_past_end
            .register_if_bound(self_id, registry, BindingLevel::Relayout);

        self.child_ids = ids.clone();

        // Set up handlers
        let scroll_y = self.scroll_y.clone();
        let scroll_x = self.scroll_x.clone();
        let max_scroll_y = self.max_scroll_y.clone();
        let max_scroll_x = self.max_scroll_x.clone();
        let viewport_size = self.viewport_size.clone();
        let viewport_origin = self.viewport_origin.clone();
        let line_height = self.line_height;
        let smooth_scrolling = self.smooth_scrolling;
        let smooth_scroll_duration = self.smooth_scroll_duration;
        let overscroll_behavior = self.overscroll_behavior;

        let clamp_and_set = {
            let scroll_y = scroll_y.clone();
            let scroll_x = scroll_x.clone();
            let max_scroll_y = max_scroll_y.clone();
            let max_scroll_x = max_scroll_x.clone();
            move || {
                let max_y = max_scroll_y.get();
                let max_x = max_scroll_x.get();
                let cur_y = scroll_y.get();
                let cur_x = scroll_x.get();
                let clamped_y = cur_y.clamp(0.0, max_y);
                let clamped_x = cur_x.clamp(0.0, max_x);
                if (clamped_y - cur_y).abs() > f32::EPSILON {
                    scroll_y.set(clamped_y);
                }
                if (clamped_x - cur_x).abs() > f32::EPSILON {
                    scroll_x.set(clamped_x);
                }
            }
        };

        let mut handlers = HandlerSet::new().clips_children(true);

        // ScrollArea stays on `on_scroll` — both mouse-wheel clicks
        // (`ScrollDelta::Lines`) and trackpad two-finger pans
        // (`ScrollDelta::Pixels`) already arrive as `WidgetEvent::Scroll`
        // from the platform, and momentum is handled by animating
        // `scroll_y`/`scroll_x` with `Easing::EaseOut` below. A future
        // touch backend would add `on_swipe` here for flick-to-scroll;
        // there is nothing to migrate today.
        //
        // Scroll handler (handles both Scroll and ScrollIntoView)
        {
            let scroll_y = scroll_y.clone();
            let scroll_x = scroll_x.clone();
            let max_scroll_y = max_scroll_y.clone();
            let max_scroll_x = max_scroll_x.clone();
            let viewport_size = viewport_size.clone();
            let viewport_origin = viewport_origin.clone();
            // Anything that scrolls this area on purpose outranks a restore that has
            // not landed yet: a reader who has started scrolling, or a caret being
            // revealed, has said where they want to be. Without this, a pending
            // offset the content is still too short to honour would be re-asserted
            // on every layout pass and fight them for it.
            let pending_restore_y = self.pending_restore_y.clone();
            let restore_wrote_y = self.restore_wrote_y.clone();
            handlers = handlers.on_scroll(move |event, _ctx| match event {
                WidgetEvent::Scroll { delta, .. } => {
                    pending_restore_y.set(None);
                    restore_wrote_y.set(None);
                    let max_y = max_scroll_y.get();
                    let max_x = max_scroll_x.get();
                    let cur_y = scroll_y.get();
                    let cur_x = scroll_x.get();
                    // Base off the animation target (not the rendered offset)
                    // so a mid-fling boundary correctly chains.
                    let base_y = scroll_y.animation_target().unwrap_or(cur_y);
                    let base_x = scroll_x.animation_target().unwrap_or(cur_x);

                    let (dx, dy) = match delta {
                        ScrollDelta::Lines { x, y } => (x * line_height, y * line_height),
                        ScrollDelta::Pixels { x, y } => (*x, *y),
                    };
                    let (target_x, moved_x) =
                        crate::common::scroll::scroll_clamp_axis(base_x, dx, max_x);
                    let (target_y, moved_y) =
                        crate::common::scroll::scroll_clamp_axis(base_y, dy, max_y);

                    if moved_x || moved_y {
                        if smooth_scrolling {
                            scroll_y.animate_to(target_y, smooth_scroll_duration, Easing::EaseOut);
                            scroll_x.animate_to(target_x, smooth_scroll_duration, Easing::EaseOut);
                        } else {
                            scroll_y.set(target_y);
                            scroll_x.set(target_x);
                        }
                    }
                    // Decline (Ignored) when fully clamped so the event chains
                    // to an ancestor scrollable, unless Contain is set.
                    crate::common::scroll::scroll_response(
                        moved_x || moved_y,
                        overscroll_behavior == OverscrollBehavior::Contain,
                    )
                }
                WidgetEvent::ScrollIntoView {
                    target_bounds,
                    margin,
                    align,
                    motion,
                    applied_scroll,
                } => {
                    pending_restore_y.set(None);
                    restore_wrote_y.set(None);
                    // `target_bounds` is in absolute tree coordinates (the
                    // arena stores screen-space rects). Convert to the
                    // content's local frame by subtracting the viewport's
                    // absolute origin and adding the current scroll offset:
                    // a child whose absolute top equals the viewport's
                    // absolute top is at content-space y = scroll_y.
                    let vp = viewport_size.get();
                    let vo = viewport_origin.get();
                    let sy = scroll_y.get();
                    let sx = scroll_x.get();

                    // Reveal on each axis independently, but leave an axis
                    // untouched when the (margin-expanded) target already spans
                    // the viewport on it: a target larger than the viewport is
                    // "as visible as it can be", and aligning one of its edges
                    // would spuriously move that axis — e.g. a full-width row
                    // (or any target as wide as the content) resetting a
                    // horizontally-scrolled ancestor on a vertical-only nav.
                    let viewport_top = sy;
                    let viewport_bottom = viewport_top + vp.height;
                    let target_top = target_bounds.y - vo.y + sy - margin;
                    let target_bottom = target_top + target_bounds.height + margin * 2.0;

                    let mut new_y = sy;
                    match align {
                        // Pin: put the target at `f` of the way down the
                        // viewport regardless of where it currently sits. The
                        // margin is deliberately not applied — a pin already
                        // names an exact position, and padding it would only
                        // shift the pin by an amount the caller did not ask for.
                        teksilo_core::event::ScrollAlign::Fraction(f) => {
                            let target_top = target_bounds.y - vo.y + sy;
                            new_y = target_top - (vp.height - target_bounds.height) * f;
                        }
                        teksilo_core::event::ScrollAlign::Minimal => {
                            if !(target_top <= viewport_top && target_bottom >= viewport_bottom) {
                                if target_top < viewport_top {
                                    new_y = target_top;
                                } else if target_bottom > viewport_bottom {
                                    new_y = target_bottom - vp.height;
                                }
                            }
                        }
                    }

                    let viewport_left = sx;
                    let viewport_right = viewport_left + vp.width;
                    let target_left = target_bounds.x - vo.x + sx - margin;
                    let target_right = target_left + target_bounds.width + margin * 2.0;

                    let mut new_x = sx;
                    if !(target_left <= viewport_left && target_right >= viewport_right) {
                        if target_left < viewport_left {
                            new_x = target_left;
                        } else if target_right > viewport_right {
                            new_x = target_right - vp.width;
                        }
                    }

                    // Clamp up front rather than setting then calling
                    // `clamp_and_set`: an animated scroll must be aimed at a
                    // reachable offset, or the tween would start toward a
                    // target the clamp immediately retracts.
                    let new_y = new_y.clamp(0.0, max_scroll_y.get());
                    let new_x = new_x.clamp(0.0, max_scroll_x.get());

                    match motion {
                        teksilo_core::event::ScrollMotion::Smooth if smooth_scrolling => {
                            scroll_y.animate_to(new_y, smooth_scroll_duration, Easing::EaseOut);
                            scroll_x.animate_to(new_x, smooth_scroll_duration, Easing::EaseOut);
                        }
                        _ => {
                            scroll_y.set(new_y);
                            scroll_x.set(new_x);
                        }
                    }
                    // Report the applied scroll delta so a nested outer
                    // container can re-target the same rect. Computed from the
                    // clamped *targets*, not the live signal, so an animated
                    // scroll reports where it is heading rather than the single
                    // frame it has travelled so far.
                    if let Some(cell) = applied_scroll
                        && let Ok(mut d) = cell.lock()
                    {
                        *d = teksilo_canvas::Point::new(new_x - sx, new_y - sy);
                    }
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            });
        }

        // Access action handler
        {
            let scroll_y = scroll_y.clone();
            let scroll_x = scroll_x.clone();
            let viewport_size = viewport_size.clone();
            let clamp_and_set = clamp_and_set.clone();
            handlers = handlers.on_access_action(move |action, _ctx| match action {
                teksilo_core::accesskit::Action::ScrollDown => {
                    let step = viewport_size.get().height * 0.9;
                    scroll_y.set(scroll_y.get() + step);
                    clamp_and_set();
                    EventResponse::Handled
                }
                teksilo_core::accesskit::Action::ScrollUp => {
                    let step = viewport_size.get().height * 0.9;
                    scroll_y.set(scroll_y.get() - step);
                    clamp_and_set();
                    EventResponse::Handled
                }
                teksilo_core::accesskit::Action::ScrollRight => {
                    let step = viewport_size.get().width * 0.9;
                    scroll_x.set(scroll_x.get() + step);
                    clamp_and_set();
                    EventResponse::Handled
                }
                teksilo_core::accesskit::Action::ScrollLeft => {
                    let step = viewport_size.get().width * 0.9;
                    scroll_x.set(scroll_x.get() - step);
                    clamp_and_set();
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            });
        }

        ctx.apply_self_handlers(handlers);

        ids
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // A scroll area's HEIGHT should come from its parent, not its content —
        // otherwise it grows to fit everything and no scrolling is needed, and
        // the intrinsic height is unstable across layout passes. Its WIDTH,
        // though, must follow the content, or a horizontally-hugging parent (a
        // menu, a popover) collapses it and clips every row.
        let (default_w, default_h) = if let Some(pref) = self.preferred_size {
            (pref.width, pref.height)
        } else {
            let h = self.preferred_height.unwrap_or(200.0);
            // `resolve()` below only ever consults `default_w` when
            // `proposal.width` is `None` — computing it otherwise measures the
            // whole content subtree via an unbounded `ctx.child_size` query
            // and then discards the result. Gate on that literal condition
            // (not on `preferred_height.is_some()`, which happens to hold for
            // the one known width-hugging caller, `menu_list.rs`, but isn't
            // the actual necessary-and-sufficient test — any other
            // `ScrollArea` under a genuinely width-hugging parent without
            // `preferred_height` set would silently regress under that
            // narrower gate).
            let w = if proposal.width.is_none() {
                self.natural_content_width(ctx)
            } else {
                0.0
            };
            (w, h)
        };
        proposal.resolve(default_w, default_h).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        if children.is_empty() {
            return;
        }

        // Children layout depends on policies:
        //   AlwaysOff  → scrollbar child exists but is collapsed to zero size
        //   AlwaysOn   → scrollbar always visible (reserves space in Permanent)
        //   AsNeeded   → visible only when content overflows
        let has_v = children.len() > 1;
        let has_h = children.len() > 2;
        let v_off = self.vertical_policy == ScrollBarPolicy::AlwaysOff;
        let _h_off = self.horizontal_policy == ScrollBarPolicy::AlwaysOff;

        // Scrollbar thickness — same for both modes (overlay paints thin at rest)
        let sb_thickness = self.scroll_bar_thickness;

        // --- Step 1: Compute viewport size (two-pass for cross-axis dependencies) ---

        // Helper: determine scrollbar visibility from policy + overflow.
        let resolve_show = |policy: ScrollBarPolicy, has_bar: bool, overflows: bool| -> bool {
            has_bar
                && match policy {
                    ScrollBarPolicy::AlwaysOn => true,
                    ScrollBarPolicy::AlwaysOff => false,
                    ScrollBarPolicy::AsNeeded => overflows,
                }
        };

        // Pass 1: measure with optimistic vertical reservation.
        let v_reserved_1 = match self.scroll_bar_style {
            ScrollBarMode::Permanent if has_v && !v_off => sb_thickness,
            _ => 0.0,
        };
        let vp_w1 = (bounds.width - v_reserved_1).max(0.0);
        let content_size_1 = ctx
            .child_size(
                children[0].id,
                SizeProposal {
                    width: Some(vp_w1),
                    height: None,
                },
            )
            .unwrap_or(Size::new(vp_w1, bounds.height));

        let show_v_1 = resolve_show(
            self.vertical_policy,
            has_v,
            content_size_1.height > bounds.height + 0.5,
        );
        let show_h_1 = resolve_show(
            self.horizontal_policy,
            has_h,
            content_size_1.width > vp_w1 + 0.5,
        );

        // Compute actual reservations from pass-1 results.
        let v_res = match self.scroll_bar_style {
            ScrollBarMode::Permanent if show_v_1 => sb_thickness,
            _ => 0.0,
        };
        let h_res = match self.scroll_bar_style {
            ScrollBarMode::Permanent if show_h_1 => sb_thickness,
            _ => 0.0,
        };

        // Pass 2: re-measure if reservations changed, and re-evaluate cross-axis.
        let vp_h_after_h = (bounds.height - h_res).max(0.0);
        let new_needs_v = content_size_1.height > vp_h_after_h + 0.5;
        let show_v = resolve_show(self.vertical_policy, has_v, new_needs_v);
        let new_v_res = match self.scroll_bar_style {
            ScrollBarMode::Permanent if show_v => sb_thickness,
            _ => 0.0,
        };

        let (viewport_width, content_size, show_h) = if (new_v_res - v_res).abs() > 0.01 {
            // Vertical reservation changed — re-measure content.
            let vp_w2 = (bounds.width - new_v_res).max(0.0);
            let cs2 = ctx
                .child_size(
                    children[0].id,
                    SizeProposal {
                        width: Some(vp_w2),
                        height: None,
                    },
                )
                .unwrap_or(Size::new(vp_w2, bounds.height));
            let sh2 = resolve_show(self.horizontal_policy, has_h, cs2.width > vp_w2 + 0.5);
            (vp_w2, cs2, sh2)
        } else {
            (
                (bounds.width - new_v_res).max(0.0),
                content_size_1,
                show_h_1,
            )
        };

        let v_reserved = new_v_res;
        let h_reserved = match self.scroll_bar_style {
            ScrollBarMode::Permanent if show_h => sb_thickness,
            _ => 0.0,
        };
        let viewport_height = (bounds.height - h_reserved).max(0.0);

        // --- Step 1b: widget_resizable — stretch content to fill viewport ---
        let placed_content_size = if self.widget_resizable {
            Size::new(
                content_size.width.max(viewport_width),
                content_size.height.max(viewport_height),
            )
        } else {
            content_size
        };

        // --- Step 2: Update shared reactive state ---
        //
        // CAUTION: this method mixes layout output with reactive-state writes.
        // It is loop-safe today because (a) the `Signal<f32>` metrics below are
        // NOT relayout-bound on the ScrollArea itself, and (b) the writes are
        // guarded so they only fire on a genuine change. If anyone ever binds
        // one of these metrics at `BindingLevel::Relayout` on the ScrollArea,
        // it becomes an instant layout loop — bind them on the scrollbar
        // children only.
        //
        // `content_size` / `viewport_size` / `viewport_origin` are `Cell`s, so
        // their `set` never notifies — written unconditionally.
        self.content_size.set(placed_content_size);
        self.viewport_size
            .set(Size::new(viewport_width, viewport_height));
        self.viewport_origin.set(bounds.origin());

        // The scrollbar children bind these `Signal<f32>` metrics for thumb
        // size/position. `Signal::set` always notifies regardless of whether
        // the value changed, so an unconditional write would re-dirty those
        // children on every relayout that reaches this node (window resize,
        // sibling content change, …) even when the metrics are identical.
        // Guard with the same EPSILON pattern as `clamp_and_set_scroll`.
        let set_if_changed = |sig: &Signal<f32>, v: f32| {
            if (sig.get() - v).abs() > f32::EPSILON {
                sig.set(v);
            }
        };

        // Scrolling past the end extends the *range* the user can reach without
        // changing the content's height. Everything downstream (the max offsets,
        // the thumb proportions) therefore works off this effective height, so
        // the scroll bar keeps telling the truth about how far there is to go.
        let past_end = (self.scroll_past_end.get().max(0.0)) * viewport_height;
        let scrollable_height = placed_content_size.height + past_end;

        let max_y = (scrollable_height - viewport_height).max(0.0);
        let max_x = (placed_content_size.width - viewport_width).max(0.0);
        set_if_changed(&self.max_scroll_y, max_y);
        set_if_changed(&self.max_scroll_x, max_x);

        let ratio_y = if scrollable_height > 0.0 {
            (viewport_height / scrollable_height).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let ratio_x = if placed_content_size.width > 0.0 {
            (viewport_width / placed_content_size.width).clamp(0.0, 1.0)
        } else {
            1.0
        };
        set_if_changed(&self.viewport_ratio_y, ratio_y);
        set_if_changed(&self.viewport_ratio_x, ratio_x);

        // A pending `restore_scroll_y` lands here, ahead of the ordinary clamp
        // below, which is what keeps the restored position from ever being
        // visible as a jump from the top.
        //
        // **It is honoured only once the range is long enough to hold it**, and
        // re-applied on every pass until then. A first nonzero range is not the
        // same thing as a measured one: a rich text editor reports its
        // `min_lines` height until its own content has been typeset, so a page
        // holding a long document grows through several passes, and taking the
        // offset on the first of them lands it clamped against a document that
        // is not there yet. That is not a near miss. Restoring 11560 into a
        // range that has reached 500 puts the reader back at the top of a
        // chapter they were at the end of, which is indistinguishable from the
        // restore never having happened.
        //
        // **And only for as long as nothing else has moved the scroll.** The
        // `on_scroll` handler stands the restore down for a wheel gesture and for
        // a `ScrollIntoView`; a scroll bar reaches neither, because it holds a
        // clone of `scroll_y` and writes it directly. Comparing against what this
        // block last wrote catches every route rather than the two that happen to
        // pass through a handler — and without it a pending offset the content is
        // *never* long enough to honour is re-asserted for the life of the widget,
        // so dragging the thumb away from the clamped bottom is undone on the very
        // next layout pass and the reader is pinned there.
        //
        // `get()` and not `animation_target()`: the only writer that gets this far
        // is a plain `set`. A wheel scroll animates, but it has already cleared the
        // pending, so an in-flight animation cannot be reached from here.
        if let Some(ours) = self.restore_wrote_y.get()
            && (self.scroll_y.get() - ours).abs() > f32::EPSILON
        {
            self.pending_restore_y.set(None);
            self.restore_wrote_y.set(None);
        }
        if let Some(pending) = self.pending_restore_y.get()
            && max_y > 0.0
        {
            let landed = pending.min(max_y);
            if (landed - self.scroll_y.get()).abs() > f32::EPSILON {
                self.scroll_y.set(landed);
            }
            if max_y >= pending {
                self.pending_restore_y.set(None);
                self.restore_wrote_y.set(None);
            } else {
                // Still short. Remember the clamped landing so the next pass can
                // tell "the content has not grown yet" from "the reader has moved".
                self.restore_wrote_y.set(Some(landed));
            }
        }

        self.clamp_and_set_scroll();
        let scroll_y = self.scroll_y.get();
        let scroll_x = self.scroll_x.get();

        // --- Step 3: Place content ---
        // RTL: anchor the content at the trailing (right) edge of the
        // bounds. With `scroll_x = 0` and content narrower than the
        // viewport, this puts the content flush-right — matching how
        // the surrounding RTL-aware stacks place their children.
        // Without this mirror, narrow content sits flush-left in both
        // directions (visible on widget-catalog tabs whose demos have
        // intrinsic widths smaller than the scroll viewport).
        let content_x = if ctx.is_rtl() {
            bounds.right() - placed_content_size.width + scroll_x
        } else {
            bounds.x - scroll_x
        };
        children[0].origin = Point::new(content_x, bounds.y - scroll_y);
        children[0].size = placed_content_size;

        // --- Step 4: Place vertical scrollbar ---
        if has_v {
            if show_v {
                let sb_x = if ctx.is_rtl() {
                    bounds.x
                } else {
                    bounds.right() - sb_thickness
                };
                let sb_h = if h_reserved > 0.0
                    || (matches!(
                        self.scroll_bar_style,
                        ScrollBarMode::Overlay | ScrollBarMode::Thin
                    ) && show_h)
                {
                    bounds.height - sb_thickness
                } else {
                    bounds.height
                };
                children[1].origin = Point::new(sb_x, bounds.y);
                children[1].size = Size::new(sb_thickness, sb_h);
            } else {
                // Collapse hidden scrollbar to zero
                children[1].origin = Point::new(bounds.x, bounds.y);
                children[1].size = Size::ZERO;
            }
        }

        // --- Step 5: Place horizontal scrollbar ---
        if has_h {
            if show_h {
                let sb_y = bounds.bottom() - sb_thickness;
                let sb_x = if ctx.is_rtl() && v_reserved > 0.0 {
                    bounds.x + sb_thickness
                } else {
                    bounds.x
                };
                let sb_w = if v_reserved > 0.0
                    || (matches!(
                        self.scroll_bar_style,
                        ScrollBarMode::Overlay | ScrollBarMode::Thin
                    ) && show_v)
                {
                    bounds.width - sb_thickness
                } else {
                    bounds.width
                };
                children[2].origin = Point::new(sb_x, sb_y);
                children[2].size = Size::new(sb_w, sb_thickness);
            } else {
                children[2].origin = Point::new(bounds.x, bounds.y);
                children[2].size = Size::ZERO;
            }
        }
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut teksilo_canvas::Canvas, _ctx: &PaintContext) {
        // ScrollBar child widgets handle all painting in both modes.
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_ids.clone()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::ScrollView);
        builder.inner_mut().set_clips_children();

        let scroll_y = self.scroll_y.get();
        let scroll_x = self.scroll_x.get();
        let max_y = self.max_scroll_y.get();
        let max_x = self.max_scroll_x.get();

        builder.inner_mut().set_scroll_y(scroll_y as f64);
        builder.inner_mut().set_scroll_y_min(0.0);
        builder.inner_mut().set_scroll_y_max(max_y as f64);
        builder.inner_mut().set_scroll_x(scroll_x as f64);
        builder.inner_mut().set_scroll_x_min(0.0);
        builder.inner_mut().set_scroll_x_max(max_x as f64);

        // Only advertise scroll actions for axes that actually overflow —
        // AT uses these to know which directions are available.
        if max_y > 0.0 {
            if scroll_y < max_y {
                builder.add_action(teksilo_core::accesskit::Action::ScrollDown);
            }
            if scroll_y > 0.0 {
                builder.add_action(teksilo_core::accesskit::Action::ScrollUp);
            }
        }
        if max_x > 0.0 {
            if scroll_x < max_x {
                builder.add_action(teksilo_core::accesskit::Action::ScrollRight);
            }
            if scroll_x > 0.0 {
                builder.add_action(teksilo_core::accesskit::Action::ScrollLeft);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_canvas::SizeProposal;
    use teksilo_core::widget::LayoutContext;
    use teksilo_core::widget_tree::WidgetTree;

    use teksilo_core::widget_builder::WidgetBuilder;

    use crate::primitives::VStack;

    /// A leaf widget with a fixed intrinsic size.
    #[derive(Debug)]
    struct TallLeaf {
        width: f32,
        height: f32,
    }

    impl TallLeaf {
        fn new(w: f32, h: f32) -> Self {
            Self {
                width: w,
                height: h,
            }
        }
    }

    impl Widget for TallLeaf {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> teksilo_core::widget::LayoutResponse {
            Size::new(
                proposal.width.unwrap_or(self.width),
                proposal.height.unwrap_or(self.height),
            )
            .into()
        }
    }

    /// A leaf whose intrinsic height can change between layout passes, standing in
    /// for a rich text editor: one reports its `min_lines` height until its own
    /// content has been typeset, so a page holding a long document grows through
    /// several passes rather than arriving at its full height on the first.
    #[derive(Debug)]
    struct GrowingLeaf {
        width: f32,
        height: Rc<Cell<f32>>,
    }

    impl GrowingLeaf {
        fn new(w: f32, height: Rc<Cell<f32>>) -> Self {
            Self { width: w, height }
        }
    }

    impl Widget for GrowingLeaf {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> teksilo_core::widget::LayoutResponse {
            Size::new(
                proposal.width.unwrap_or(self.width),
                proposal.height.unwrap_or(self.height.get()),
            )
            .into()
        }
    }

    #[test]
    fn scroll_area_clips_hit_test() {
        let mut tree = WidgetTree::new();

        // Content taller than viewport: 3 items x 100px = 300px
        let a = tree.add(TallLeaf::new(200.0, 100.0));
        let b = tree.add(TallLeaf::new(200.0, 100.0));
        let c = tree.add(TallLeaf::new(200.0, 100.0));
        let content = tree.add(VStack::new().add_child(a).add_child(b).add_child(c));

        let scroll = tree.add(ScrollArea::from_id(content));

        // Viewport is 200x80 — only first 80px visible
        tree.layout(SizeProposal::exact(200.0, 80.0));

        // Point inside viewport: should hit a child
        let hit = tree.hit_test(Point::new(50.0, 40.0));
        assert!(hit.is_some());

        // Point outside viewport (below): should not hit any child
        let hit_outside = tree.hit_test(Point::new(50.0, 100.0));
        // This point is outside the scroll area's 80px bounds
        assert!(hit_outside.is_none() || hit_outside == Some(scroll));
    }

    #[test]
    fn scroll_changes_visible_content() {
        let mut tree = WidgetTree::new();

        let a = tree.add(TallLeaf::new(200.0, 100.0));
        let b = tree.add(TallLeaf::new(200.0, 100.0));
        let content = tree.add(VStack::new().add_child(a).add_child(b));

        let _scroll = tree.add(ScrollArea::from_id(content).smooth_scrolling(false));

        tree.layout(SizeProposal::exact(200.0, 80.0));

        // Before scrolling, item a is at y=0
        assert!(tree.bounds(a).y >= 0.0);

        // Move pointer into viewport so Scroll events have a target
        tree.pointer_move(Point::new(50.0, 40.0));

        // Scroll down 100px
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 100.0 },
            modifiers: Default::default(),
        });
        tree.layout(SizeProposal::exact(200.0, 80.0));

        // After scrolling, item a should be above viewport (negative y)
        assert!(tree.bounds(a).y < 0.0);
        // Item b should now be at or near viewport top
        assert!(tree.bounds(b).y < 80.0);
    }

    #[test]
    fn scroll_accessibility_reports_position() {
        let mut tree = WidgetTree::new();
        let content = tree.add(TallLeaf::new(200.0, 1000.0));
        let scroll = tree.add(ScrollArea::from_id(content));

        tree.layout(SizeProposal::exact(200.0, 80.0));

        let info = tree.accessibility_node(scroll);
        assert_eq!(info.role(), teksilo_core::accesskit::Role::ScrollView);
    }

    #[test]
    fn scroll_offset_is_clamped() {
        let mut tree = WidgetTree::new();
        let content = tree.add(TallLeaf::new(200.0, 200.0));
        let _scroll = tree.add(ScrollArea::from_id(content).smooth_scrolling(false));

        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Move pointer into viewport
        tree.pointer_move(Point::new(50.0, 50.0));

        // Scroll way past the end
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 9999.0 },
            modifiers: Default::default(),
        });
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Content should not be scrolled past max (200 - 100 = 100)
        let content_y = tree.bounds(content).y;
        assert!(content_y >= -100.0 - 0.01);
    }

    #[test]
    fn permanent_scrollbar_reduces_viewport() {
        let mut tree = WidgetTree::new();

        let content = TallLeaf::new(200.0, 500.0);
        let scroll = tree.add(
            ScrollArea::new()
                .child(content)
                .scroll_bar_style(ScrollBarMode::Permanent)
                .scroll_bar_thickness(12.0),
        );

        tree.layout(SizeProposal::exact(200.0, 100.0));

        // The scroll area should be 200x100
        let scroll_bounds = tree.bounds(scroll);
        assert!((scroll_bounds.width - 200.0).abs() < 0.01);
        assert!((scroll_bounds.height - 100.0).abs() < 0.01);
    }

    #[test]
    fn permanent_scrollbar_scroll_event_updates_content() {
        let mut tree = WidgetTree::new();

        let leaf = TallLeaf::new(180.0, 500.0);
        let scroll = tree.add(
            ScrollArea::new()
                .child(leaf)
                .scroll_bar_style(ScrollBarMode::Permanent)
                .smooth_scrolling(false),
        );

        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Scroll via mouse wheel
        tree.pointer_move(Point::new(50.0, 50.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 50.0 },
            modifiers: Default::default(),
        });
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // The content child should have moved up
        let children = tree.children(scroll);
        assert!(!children.is_empty());
        let content_y = tree.bounds(children[0]).y;
        assert!(
            content_y < 0.0,
            "Expected negative y after scroll, got {}",
            content_y
        );
    }

    #[test]
    fn overlay_mode_has_scrollbar_children() {
        let mut tree = WidgetTree::new();
        let content = tree.add(TallLeaf::new(200.0, 500.0));
        let scroll = tree.add(ScrollArea::from_id(content));

        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Overlay mode has 3 children: content + v_scrollbar + h_scrollbar
        let children = tree.children(scroll);
        assert_eq!(children.len(), 3, "Overlay mode should have 3 children");

        // Viewport uses full width (no space reserved for scrollbar)
        let content_bounds = tree.bounds(children[0]);
        assert!(
            (content_bounds.width - 200.0).abs() < 0.01,
            "Overlay mode should not shrink viewport"
        );
    }

    #[test]
    fn scroll_area_new_accepts_inline_widget() {
        let mut tree = WidgetTree::new();
        // Test the new API: pass widget directly, not a WidgetId
        let scroll = tree.add(ScrollArea::new().child(TallLeaf::new(200.0, 500.0)));

        tree.layout(SizeProposal::exact(200.0, 100.0));

        let bounds = tree.bounds(scroll);
        assert!((bounds.width - 200.0).abs() < 0.01);
    }

    /// A leaf widget that always reports its intrinsic size, ignoring proposals.
    #[derive(Debug)]
    struct WideLeaf {
        width: f32,
        height: f32,
    }
    impl WideLeaf {
        fn new(w: f32, h: f32) -> Self {
            Self {
                width: w,
                height: h,
            }
        }
    }
    impl Widget for WideLeaf {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> teksilo_core::widget::LayoutResponse {
            Size::new(self.width, self.height).into()
        }
    }

    #[test]
    fn permanent_horizontal_scrollbar_present() {
        let mut tree = WidgetTree::new();
        // Content wider and taller than viewport
        let scroll = tree.add(
            ScrollArea::new()
                .child(WideLeaf::new(400.0, 500.0))
                .scroll_bar_style(ScrollBarMode::Permanent)
                .scroll_bar_thickness(12.0),
        );

        tree.layout(SizeProposal::exact(200.0, 100.0));

        let children = tree.children(scroll);
        assert_eq!(
            children.len(),
            3,
            "Permanent mode should have content + v_sb + h_sb"
        );

        // Vertical scrollbar: right edge, height = bounds.height - h_sb_thickness
        let v_sb = tree.bounds(children[1]);
        assert!((v_sb.width - 12.0).abs() < 0.01, "v_sb width should be 12");
        assert!((v_sb.x - (200.0 - 12.0)).abs() < 0.01, "v_sb at right edge");
        assert!(
            (v_sb.height - (100.0 - 12.0)).abs() < 0.01,
            "v_sb height reduced by h_sb thickness, got {}",
            v_sb.height
        );

        // Horizontal scrollbar: bottom edge, width = viewport_width
        let h_sb = tree.bounds(children[2]);
        assert!(
            (h_sb.height - 12.0).abs() < 0.01,
            "h_sb height should be 12"
        );
        assert!(
            (h_sb.y - (100.0 - 12.0)).abs() < 0.01,
            "h_sb at bottom edge"
        );
        assert!(
            (h_sb.width - (200.0 - 12.0)).abs() < 0.01,
            "h_sb width = bounds.width - v_sb, got {}",
            h_sb.width
        );
    }

    #[test]
    fn permanent_no_horizontal_when_content_fits() {
        let mut tree = WidgetTree::new();
        // Content taller but NOT wider than viewport (accounting for v_sb)
        let scroll = tree.add(
            ScrollArea::new()
                .child(TallLeaf::new(180.0, 500.0))
                .scroll_bar_style(ScrollBarMode::Permanent)
                .scroll_bar_thickness(12.0),
        );

        tree.layout(SizeProposal::exact(200.0, 100.0));

        let children = tree.children(scroll);
        assert_eq!(children.len(), 3);

        // Horizontal scrollbar still exists as child but max_scroll_x == 0
        // so it paints nothing. No space reserved vertically.
        let v_sb = tree.bounds(children[1]);
        assert!(
            (v_sb.height - 100.0).abs() < 0.01,
            "v_sb should use full height when no h-scroll needed, got {}",
            v_sb.height
        );
    }

    #[test]
    fn overlay_scrollbar_does_not_reduce_viewport() {
        let mut tree = WidgetTree::new();
        let scroll = tree.add(
            ScrollArea::new()
                .child(WideLeaf::new(400.0, 500.0))
                .scroll_bar_style(ScrollBarMode::Overlay),
        );

        tree.layout(SizeProposal::exact(200.0, 100.0));

        let children = tree.children(scroll);
        assert_eq!(children.len(), 3);

        // Content should use full width (overlay doesn't shrink viewport)
        let content = tree.bounds(children[0]);
        assert!(
            content.width >= 400.0,
            "Content should report its full intrinsic width, got {}",
            content.width
        );

        // Vertical scrollbar overlays the right edge (full thickness, paints thin at rest)
        let v_sb = tree.bounds(children[1]);
        assert!(
            (v_sb.width - 12.0).abs() < 0.01,
            "Overlay v_sb should have full thickness for hover expansion, got {}",
            v_sb.width
        );
        assert!(
            (v_sb.x - (200.0 - 12.0)).abs() < 0.01,
            "Overlay v_sb at right edge"
        );

        // Horizontal scrollbar overlays the bottom edge
        let h_sb = tree.bounds(children[2]);
        assert!(
            (h_sb.height - 12.0).abs() < 0.01,
            "Overlay h_sb should have full thickness for hover expansion, got {}",
            h_sb.height
        );
        assert!(
            (h_sb.y - (100.0 - 12.0)).abs() < 0.01,
            "Overlay h_sb at bottom edge"
        );
    }

    #[test]
    fn horizontal_scroll_via_wheel() {
        let mut tree = WidgetTree::new();
        let scroll = tree.add(
            ScrollArea::new()
                .child(WideLeaf::new(400.0, 100.0))
                .scroll_bar_style(ScrollBarMode::Permanent)
                .scroll_bar_thickness(12.0)
                .smooth_scrolling(false),
        );

        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.pointer_move(Point::new(50.0, 50.0));

        // Scroll right via horizontal wheel
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 80.0, y: 0.0 },
            modifiers: Default::default(),
        });
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Content should have shifted left
        let children = tree.children(scroll);
        let content_x = tree.bounds(children[0]).x;
        assert!(
            content_x < 0.0,
            "Expected negative x after h-scroll, got {}",
            content_x
        );
    }

    // --- ScrollBarPolicy tests ---

    #[test]
    fn vertical_scrollbar_always_off_hides_scrollbar() {
        let mut tree = WidgetTree::new();
        let scroll = tree.add(
            ScrollArea::new()
                .child(TallLeaf::new(200.0, 500.0))
                .scroll_bar_style(ScrollBarMode::Permanent)
                .vertical_scroll_bar_policy(ScrollBarPolicy::AlwaysOff)
                .scroll_bar_thickness(12.0),
        );

        tree.layout(SizeProposal::exact(200.0, 100.0));

        let children = tree.children(scroll);
        // v_scrollbar should be collapsed to zero
        let v_sb = tree.bounds(children[1]);
        assert!(
            (v_sb.width).abs() < 0.01,
            "v_sb should be zero-width, got {}",
            v_sb.width
        );
        assert!(
            (v_sb.height).abs() < 0.01,
            "v_sb should be zero-height, got {}",
            v_sb.height
        );

        // Content should use full width (no space reserved)
        let content = tree.bounds(children[0]);
        assert!(
            (content.width - 200.0).abs() < 0.01,
            "Content should use full width when v_sb is off, got {}",
            content.width
        );
    }

    #[test]
    fn horizontal_scrollbar_always_off_hides_scrollbar() {
        let mut tree = WidgetTree::new();
        let scroll = tree.add(
            ScrollArea::new()
                .child(WideLeaf::new(400.0, 500.0))
                .scroll_bar_style(ScrollBarMode::Permanent)
                .horizontal_scroll_bar_policy(ScrollBarPolicy::AlwaysOff)
                .scroll_bar_thickness(12.0),
        );

        tree.layout(SizeProposal::exact(200.0, 100.0));

        let children = tree.children(scroll);
        // h_scrollbar should be collapsed to zero
        let h_sb = tree.bounds(children[2]);
        assert!(
            (h_sb.width).abs() < 0.01,
            "h_sb should be zero-width, got {}",
            h_sb.width
        );

        // v_scrollbar should use full height (no h_sb reservation)
        let v_sb = tree.bounds(children[1]);
        assert!(
            (v_sb.height - 100.0).abs() < 0.01,
            "v_sb should use full height when h_sb off, got {}",
            v_sb.height
        );
    }

    #[test]
    fn scrollbar_always_on_shows_even_when_content_fits() {
        let mut tree = WidgetTree::new();
        // Content fits in viewport — normally scrollbar would hide
        let scroll = tree.add(
            ScrollArea::new()
                .child(TallLeaf::new(100.0, 50.0))
                .scroll_bar_style(ScrollBarMode::Permanent)
                .vertical_scroll_bar_policy(ScrollBarPolicy::AlwaysOn)
                .scroll_bar_thickness(12.0),
        );

        tree.layout(SizeProposal::exact(200.0, 100.0));

        let children = tree.children(scroll);
        let v_sb = tree.bounds(children[1]);
        // Scrollbar should be visible despite content fitting
        assert!(
            (v_sb.width - 12.0).abs() < 0.01,
            "v_sb should be visible (12px) even when content fits, got {}",
            v_sb.width
        );
    }

    // --- widget_resizable tests ---

    #[test]
    fn widget_resizable_stretches_small_content() {
        let mut tree = WidgetTree::new();
        // Content is 100x50, viewport is 200x100
        let scroll = tree.add(
            ScrollArea::new()
                .child(TallLeaf::new(100.0, 50.0))
                .widget_resizable(true),
        );

        tree.layout(SizeProposal::exact(200.0, 100.0));

        let children = tree.children(scroll);
        let content = tree.bounds(children[0]);
        // Content should be stretched to fill viewport
        assert!(
            content.width >= 200.0 - 0.01,
            "Resizable content width should fill viewport, got {}",
            content.width
        );
        assert!(
            content.height >= 100.0 - 0.01,
            "Resizable content height should fill viewport, got {}",
            content.height
        );
    }

    #[test]
    fn widget_resizable_does_not_shrink_large_content() {
        let mut tree = WidgetTree::new();
        // Content is larger than viewport
        let scroll = tree.add(
            ScrollArea::new()
                .child(WideLeaf::new(400.0, 500.0))
                .widget_resizable(true),
        );

        tree.layout(SizeProposal::exact(200.0, 100.0));

        let children = tree.children(scroll);
        let content = tree.bounds(children[0]);
        assert!(
            content.width >= 400.0 - 0.01,
            "Large content should not be shrunk, got {}",
            content.width
        );
        assert!(
            content.height >= 500.0 - 0.01,
            "Large content should not be shrunk, got {}",
            content.height
        );
    }

    // --- smooth scrolling tests ---

    #[test]
    fn smooth_scrolling_line_events_use_animation() {
        let mut tree = WidgetTree::new();
        let scroll = tree.add(
            ScrollArea::new()
                .child(TallLeaf::new(200.0, 1000.0))
                .smooth_scrolling(true),
        );

        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.pointer_move(Point::new(50.0, 50.0));

        // Scroll via line-based wheel (should animate)
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Lines { x: 0.0, y: 5.0 },
            modifiers: Default::default(),
        });

        // The animation target was set but not yet ticked — the state
        // should have a pending animation (animate_to marks dirty).
        // After a layout + tick, the value should be moving toward the target.
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Tick part of the animation
        tree.tick_animations(Duration::from_millis(75));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let children = tree.children(scroll);
        let content_y = tree.bounds(children[0]).y;
        // Should have scrolled partially (target = 5 * 20 = 100px)
        assert!(
            content_y < 0.0,
            "Expected partial scroll, got y={}",
            content_y
        );
        assert!(
            content_y > -100.0,
            "Should not have reached target yet, got y={}",
            content_y
        );
    }

    #[test]
    fn smooth_scrolling_disabled_jumps_immediately() {
        let mut tree = WidgetTree::new();
        let scroll = tree.add(
            ScrollArea::new()
                .child(TallLeaf::new(200.0, 1000.0))
                .smooth_scrolling(false),
        );

        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.pointer_move(Point::new(50.0, 50.0));

        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Lines { x: 0.0, y: 5.0 },
            modifiers: Default::default(),
        });
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let children = tree.children(scroll);
        let content_y = tree.bounds(children[0]).y;
        // Should jump immediately to target (5 * 20 = 100px)
        assert!(
            (content_y - (-100.0)).abs() < 0.01,
            "Should jump immediately, got y={}",
            content_y
        );
    }

    // --- preferred_size tests ---

    #[test]
    fn preferred_size_overrides_default() {
        let mut tree = WidgetTree::new();
        let scroll = tree.add(
            ScrollArea::new()
                .child(TallLeaf::new(200.0, 500.0))
                .preferred_size(500.0, 400.0),
        );
        // With unconstrained proposal, should use preferred size
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        let bounds = tree.bounds(scroll);
        assert!(
            (bounds.width - 500.0).abs() < 0.01,
            "Should use preferred width, got {}",
            bounds.width
        );
        assert!(
            (bounds.height - 400.0).abs() < 0.01,
            "Should use preferred height, got {}",
            bounds.height
        );
    }

    #[test]
    fn constrained_proposal_overrides_preferred_size() {
        let mut tree = WidgetTree::new();
        let scroll = tree.add(
            ScrollArea::new()
                .child(TallLeaf::new(200.0, 500.0))
                .preferred_size(500.0, 400.0),
        );
        // With constrained proposal, the proposal wins
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let bounds = tree.bounds(scroll);
        assert!((bounds.width - 200.0).abs() < 0.01);
        assert!((bounds.height - 100.0).abs() < 0.01);
    }

    // --- theme/locale rebuild should not reset scroll offset ---

    #[test]
    fn scroll_survives_theme_switch_at_root() {
        let mut tree = WidgetTree::new();
        let scroll = tree.add(
            ScrollArea::new()
                .child(TallLeaf::new(200.0, 500.0))
                .smooth_scrolling(false),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Scroll partway down
        tree.pointer_move(Point::new(50.0, 50.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 150.0 },
            modifiers: Default::default(),
        });
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let content = tree.children(scroll)[0];
        let content_y_before = tree.bounds(content).y;
        assert!(
            content_y_before < -100.0,
            "Content should have scrolled; got y={}",
            content_y_before
        );

        // Switch theme — should NOT reset scroll
        tree.set_theme(teksilo_core::presets::intui::dark());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let content = tree.children(scroll)[0];
        let content_y_after = tree.bounds(content).y;
        assert!(
            (content_y_after - content_y_before).abs() < 0.01,
            "Scroll offset should survive theme switch: before={}, after={}",
            content_y_before,
            content_y_after
        );
    }

    /// Composite parent that wraps a ScrollArea via ctx.add(ScrollArea::new()...).
    /// Simulates a typical user widget: its build() runs on every theme change,
    /// so a naive ScrollArea::new() inside would lose its scroll offset.
    #[derive(Debug)]
    struct ScrollParent {
        scroll_id: Option<WidgetId>,
    }
    impl ScrollParent {
        fn new() -> Self {
            Self { scroll_id: None }
        }
    }
    impl Widget for ScrollParent {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let id = ctx.add(
                ScrollArea::new()
                    .child(TallLeaf::new(200.0, 500.0))
                    .smooth_scrolling(false),
            );
            self.scroll_id = Some(id);
            vec![id]
        }
        fn layout_response(
            &self,
            proposal: SizeProposal,
            ctx: &LayoutContext,
        ) -> teksilo_core::widget::LayoutResponse {
            self.scroll_id
                .and_then(|id| ctx.child_size(id, proposal))
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
                .into()
        }
        fn place_children(
            &self,
            bounds: Rect,
            _proposal: SizeProposal,
            children: &mut [WidgetPlacement],
            _ctx: &LayoutContext,
        ) {
            if let Some(child) = children.first_mut() {
                child.origin = bounds.origin();
                child.size = bounds.size();
            }
        }
    }

    #[test]
    fn scroll_survives_theme_switch_inside_composite() {
        let mut tree = WidgetTree::new();
        let parent = tree.add(ScrollParent::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.pointer_move(Point::new(50.0, 50.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 150.0 },
            modifiers: Default::default(),
        });
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let scroll_before = tree.children(parent)[0];
        let content_before = tree.children(scroll_before)[0];
        let y_before = tree.bounds(content_before).y;
        assert!(
            y_before < -100.0,
            "Content should have scrolled; got y={}",
            y_before
        );

        tree.set_theme(teksilo_core::presets::intui::dark());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let scroll_after = tree.children(parent)[0];
        let content_after = tree.children(scroll_after)[0];
        let y_after = tree.bounds(content_after).y;
        assert!(
            (y_after - y_before).abs() < 0.01,
            "Scroll offset should survive theme switch inside composite: before={}, after={}",
            y_before,
            y_after
        );
    }

    // --- ScrollIntoView regression: focused widget above viewport ---

    /// Regression: when the focused widget is above the viewport top and
    /// the ScrollArea is itself offset from the tree origin, focusing the
    /// widget should scroll *up* (decreasing scroll_y) to bring it back
    /// into view — not *down*. The earlier implementation treated
    /// `target_bounds` as if it were already viewport-relative and added
    /// `scroll_y` to it, which scrolled past the widget when the
    /// ScrollArea was not at absolute (0, 0). Cloning a `Cell` also
    /// produces an independent cell, so the closure was reading a stale
    /// `viewport_size = Size::ZERO`; both must be fixed for the math to
    /// produce the right answer.
    #[test]
    fn scroll_into_view_brings_widget_above_viewport_into_view() {
        let mut tree = WidgetTree::new();

        // Layout: VStack { 50px header, ScrollArea(content 500px) }.
        // Total height 250 → ScrollArea bounds.y = 50 (the offset that
        // previously triggered the bug).
        let header = tree.add(TallLeaf::new(200.0, 50.0));
        // Focusable target near the top of the content.
        let target = tree.add(TallLeaf::new(200.0, 20.0).focusable(true));
        let after = tree.add(TallLeaf::new(200.0, 470.0));
        let content = tree.add(VStack::new().add_child(target).add_child(after));
        let scroll = tree.add(ScrollArea::from_id(content).smooth_scrolling(false));
        let _root = tree.add(VStack::new().add_child(header).add_child(scroll));

        tree.layout(SizeProposal::exact(200.0, 250.0));

        let scroll_bounds = tree.bounds(scroll);
        assert!(
            (scroll_bounds.y - 50.0).abs() < 0.01,
            "ScrollArea should sit below the header at y=50, got {}",
            scroll_bounds.y
        );

        // Scroll down so the target is well above the viewport top.
        tree.pointer_move(Point::new(100.0, 100.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 150.0 },
            modifiers: Default::default(),
        });
        tree.layout(SizeProposal::exact(200.0, 250.0));

        let target_before = tree.bounds(target);
        assert!(
            target_before.bottom() < scroll_bounds.y,
            "Target should be above viewport before focus, got y={} (viewport top={})",
            target_before.y,
            scroll_bounds.y
        );

        // Focus the target — fires ScrollIntoView, should bring the
        // widget back into view rather than push it further away.
        tree.focus(target);
        tree.layout(SizeProposal::exact(200.0, 250.0));

        let target_after = tree.bounds(target);
        let viewport_top = scroll_bounds.y;
        let viewport_bottom = scroll_bounds.bottom();
        assert!(
            target_after.y >= viewport_top - 0.5 && target_after.bottom() <= viewport_bottom + 0.5,
            "Target should be inside viewport after focus, got y={}..{} (viewport={}..{})",
            target_after.y,
            target_after.bottom(),
            viewport_top,
            viewport_bottom
        );
    }

    // --- Typewriter scrolling: alignment + scroll-past-end ------------------

    /// A `ScrollArea` over `content_h` of content, laid out at `200 x
    /// viewport_h`, with a focused child inside it that issues an aligned
    /// reveal when it receives a key.
    ///
    /// The request goes through the real path — `EventContext` →
    /// `collect_from_ctx` → the clipping-ancestor walk → the area's handler —
    /// because `ScrollIntoView` is deliberately inert in the top-level event
    /// router and only ever arrives that way.
    struct PinFixture {
        tree: WidgetTree,
        bounds: Rect,
        viewport_h: f32,
        scroll_y: Signal<f32>,
        max_scroll_y: Signal<f32>,
        ratio_y: Signal<f32>,
        /// The rect the actor will ask to have pinned, in window space, plus the
        /// fraction to pin it at. Rewritten before each key.
        request: Rc<Cell<(Rect, f32)>>,
    }

    fn pin_fixture(content_h: f32, viewport_h: f32, past_end: f32) -> PinFixture {
        let request = Rc::new(Cell::new((Rect::new(0.0, 0.0, 0.0, 0.0), 0.5)));
        let mut tree = WidgetTree::new();

        let req = request.clone();
        let actor = tree.add(TallLeaf::new(200.0, content_h).focusable(true).on_key(
            move |_ev, ctx| {
                let (rect, fraction) = req.get();
                ctx.ensure_visible_aligned(
                    rect,
                    fraction,
                    teksilo_core::event::ScrollMotion::Instant,
                );
                EventResponse::Handled
            },
        ));
        let content = tree.add(VStack::new().add_child(actor));
        let sa = ScrollArea::from_id(content)
            .smooth_scrolling(false)
            .scroll_past_end(past_end);
        let scroll_y = sa.scroll_y_signal().clone();
        let max_scroll_y = sa.max_scroll_y_signal().clone();
        let ratio_y = sa.viewport_ratio_y_signal().clone();
        let scroll = tree.add(sa);
        tree.layout(SizeProposal::exact(200.0, viewport_h));
        tree.focus(actor);
        // Focusing fires a Minimal reveal of the (viewport-sized) actor; settle
        // it before the tests measure.
        tree.layout(SizeProposal::exact(200.0, viewport_h));
        scroll_y.set(0.0);

        let bounds = tree.bounds(scroll);
        PinFixture {
            tree,
            bounds,
            viewport_h,
            scroll_y,
            max_scroll_y,
            ratio_y,
            request,
        }
    }

    impl PinFixture {
        /// Pin a `height`-tall line whose top sits at `content_y` in content
        /// space, at `fraction` of the viewport.
        fn pin(&mut self, content_y: f32, height: f32, fraction: f32) {
            let window_y = self.bounds.y + content_y - self.scroll_y.get();
            self.request
                .set((Rect::new(0.0, window_y, 200.0, height), fraction));
            self.tree.dispatch_event(WidgetEvent::KeyDown {
                key: teksilo_core::event::Key::ArrowDown,
                modifiers: Default::default(),
                text: None,
            });
            self.tree
                .layout(SizeProposal::exact(200.0, self.viewport_h));
        }
    }

    #[test]
    fn scroll_past_end_extends_the_range_without_changing_the_content() {
        // 300px of content in a 100px viewport scrolls 200px normally.
        let plain = pin_fixture(300.0, 100.0, 0.0);
        assert_eq!(plain.max_scroll_y.get(), 200.0);

        // Half a viewport past the end buys exactly 50px more.
        let padded = pin_fixture(300.0, 100.0, 0.5);
        assert_eq!(
            padded.max_scroll_y.get(),
            250.0,
            "scroll_past_end(0.5) must add half a viewport of range"
        );
    }

    #[test]
    fn scroll_past_end_keeps_the_thumb_proportional() {
        // The thumb must size against the range the user can actually travel,
        // or the scroll bar claims there is less document left than there is.
        let f = pin_fixture(300.0, 100.0, 0.5);
        // Effective scrollable height is 300 + 50 = 350.
        let expected = 100.0 / 350.0;
        assert!(
            (f.ratio_y.get() - expected).abs() < 1e-4,
            "thumb ratio must use the extended range, got {}",
            f.ratio_y.get()
        );
    }

    #[test]
    fn scroll_past_end_lets_the_last_line_reach_a_centre_pin() {
        // The case that motivates the feature: a line at the very bottom of the
        // content cannot reach the middle of the viewport without range past
        // the end — and that is exactly where a writer spends their time.
        let mut f = pin_fixture(300.0, 100.0, 0.5);
        f.pin(280.0, 20.0, 0.5);

        // Centring a 20px line in a 100px viewport puts its top at 40px, so the
        // offset must be 280 - 40 = 240 — reachable only because
        // scroll_past_end(0.5) raised the maximum from 200 to 250.
        assert_eq!(
            f.scroll_y.get(),
            240.0,
            "the last line must be able to sit at the pin"
        );
    }

    #[test]
    fn without_scroll_past_end_the_last_line_cannot_reach_the_pin() {
        // The negative control for the test above: same geometry, no extra
        // range, so the pin is clamped short and the line stays at the bottom.
        let mut f = pin_fixture(300.0, 100.0, 0.0);
        f.pin(280.0, 20.0, 0.5);
        assert_eq!(
            f.scroll_y.get(),
            200.0,
            "clamped to the un-extended maximum"
        );
    }

    #[test]
    fn a_pin_near_the_document_start_clamps_instead_of_scrolling_negative() {
        // Deliberate design choice: no padding above the content, so the caret
        // rides above the pin until there is room to honour it.
        let mut f = pin_fixture(300.0, 100.0, 0.5);
        f.pin(0.0, 20.0, 0.5);
        assert_eq!(
            f.scroll_y.get(),
            0.0,
            "the first line must clamp at the top, never scroll past it"
        );
    }

    #[test]
    fn a_fraction_pin_places_the_target_at_that_height() {
        // 0.25 → the target's top sits a quarter of the way down the free space.
        let mut f = pin_fixture(600.0, 100.0, 0.0);
        f.pin(300.0, 20.0, 0.25);
        // Free space = 100 - 20 = 80; a quarter of that is 20 → offset 280.
        assert_eq!(f.scroll_y.get(), 280.0);
    }

    #[test]
    fn a_pin_re_asserts_on_an_already_visible_target() {
        // The property that separates a pin from a reveal. Park the view so the
        // target is comfortably on screen, then pin it and check the view still
        // moved to put it exactly on the mark.
        let mut f = pin_fixture(600.0, 100.0, 0.0);
        f.scroll_y.set(250.0);
        f.tree.layout(SizeProposal::exact(200.0, 100.0));

        // Content-space 300 is visible at offset 250 (50px down the viewport).
        f.pin(300.0, 20.0, 0.5);

        assert_eq!(
            f.scroll_y.get(),
            260.0,
            "a pin must move an already-visible target onto the mark"
        );
    }

    #[test]
    fn scroll_into_view_reveals_target_through_two_nested_scroll_areas() {
        // A focusable target sits deep inside an INNER ScrollArea, which is
        // itself below the fold of an OUTER ScrollArea — both must scroll to
        // reveal it. The inner reports its applied scroll through the
        // `applied_scroll` back-channel so the outer targets where the child
        // *lands* (post-inner-scroll), not its stale pre-scroll position. The
        // end-to-end check is that the target is actually visible after one pass.
        use crate::primitives::FixedSize;

        let mut tree = WidgetTree::new();
        // Inner content: 200px spacer, the 20px target, 100px tail → 320px.
        let target = tree.add(TallLeaf::new(200.0, 20.0).focusable(true));
        let inner_spacer = tree.add(TallLeaf::new(200.0, 200.0));
        let inner_tail = tree.add(TallLeaf::new(200.0, 100.0));
        let inner_content = tree.add(
            VStack::new()
                .add_child(inner_spacer)
                .add_child(target)
                .add_child(inner_tail),
        );
        let inner_sa = tree.add(ScrollArea::from_id(inner_content).smooth_scrolling(false));
        // Bound the inner ScrollArea to an 80px viewport.
        let inner_box = tree.add(
            FixedSize::new()
                .width(200.0)
                .height(80.0)
                .child_id(inner_sa),
        );
        // Outer content: 200px spacer, the inner box (below the fold), 200px tail.
        let outer_spacer = tree.add(TallLeaf::new(200.0, 200.0));
        let outer_tail = tree.add(TallLeaf::new(200.0, 200.0));
        let outer_content = tree.add(
            VStack::new()
                .add_child(outer_spacer)
                .add_child(inner_box)
                .add_child(outer_tail),
        );
        let outer_sa = tree.add(ScrollArea::from_id(outer_content).smooth_scrolling(false));

        // Outer viewport is 100px tall; the inner box starts at y≈200 → below it.
        let sz = SizeProposal::exact(200.0, 100.0);
        tree.layout(sz);

        // Focus the deeply-nested target → walks both ScrollAreas.
        tree.focus(target);
        tree.layout(sz);

        let outer_bounds = tree.bounds(outer_sa);
        let t = tree.bounds(target);
        assert!(
            t.y >= outer_bounds.y - 1.0 && t.bottom() <= outer_bounds.bottom() + 1.0,
            "target must be visible in the outer window after both scroll: target y={}..{}, \
             outer viewport {}..{}",
            t.y,
            t.bottom(),
            outer_bounds.y,
            outer_bounds.bottom()
        );
    }

    /// A leaf with a fixed intrinsic size that ignores the proposal —
    /// needed to test ScrollArea behavior with content narrower than
    /// the viewport. `TallLeaf` accepts the proposed width, which would
    /// always make content match viewport width and hide RTL bugs.
    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> teksilo_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    #[test]
    fn rtl_anchors_narrow_content_to_trailing_edge() {
        // Reproduces the widget-catalog "tab content pushed left in RTL"
        // bug: a ScrollArea wrapping content narrower than the viewport
        // used to place the content at bounds.x in both directions.
        let mut tree = WidgetTree::new();
        let content = tree.add(FixedLeaf(120.0, 80.0));
        let _scroll = tree.add(ScrollArea::from_id(content));

        tree.set_layout_direction(teksilo_core::environment::LayoutDirection::RightToLeft);
        tree.layout(SizeProposal::exact(400.0, 200.0));

        let cb = tree.bounds(content);
        assert!(
            (cb.x - (400.0 - 120.0)).abs() < 0.01,
            "RTL content should be flush-right at x=280, got {}",
            cb.x
        );
    }

    #[test]
    fn ltr_anchors_narrow_content_to_leading_edge() {
        let mut tree = WidgetTree::new();
        let content = tree.add(FixedLeaf(120.0, 80.0));
        let _scroll = tree.add(ScrollArea::from_id(content));

        tree.layout(SizeProposal::exact(400.0, 200.0));

        let cb = tree.bounds(content);
        assert!(
            cb.x.abs() < 0.01,
            "LTR content should be flush-left at x=0, got {}",
            cb.x
        );
    }

    /// Build an outer ScrollArea whose content is `[inner ScrollArea (100px
    /// viewport, 300px content), 200px filler]` in a 150px outer viewport.
    /// Returns `(tree, inner_scroll_y, outer_scroll_y)`.
    fn nested_scroll_fixture(
        inner_overscroll: OverscrollBehavior,
    ) -> (WidgetTree, Signal<f32>, Signal<f32>) {
        let mut tree = WidgetTree::new();

        let inner_content = tree.add(TallLeaf::new(200.0, 300.0));
        let inner_sa = ScrollArea::from_id(inner_content)
            .smooth_scrolling(false)
            .preferred_size(200.0, 100.0)
            .overscroll_behavior(inner_overscroll);
        let inner_y = inner_sa.scroll_y_signal().clone();
        let inner = tree.add(inner_sa);

        let filler = tree.add(TallLeaf::new(200.0, 200.0));
        let outer_content = tree.add(VStack::new().add_child(inner).add_child(filler));
        let outer_sa = ScrollArea::from_id(outer_content).smooth_scrolling(false);
        let outer_y = outer_sa.scroll_y_signal().clone();
        let _outer = tree.add(outer_sa);

        tree.layout(SizeProposal::exact(200.0, 150.0));
        (tree, inner_y, outer_y)
    }

    #[test]
    fn nested_scroll_chains_to_outer_at_boundary() {
        let (mut tree, inner_y, outer_y) = nested_scroll_fixture(OverscrollBehavior::Chain);

        // Pointer over the inner viewport, then scroll the inner to its bottom.
        tree.pointer_move(Point::new(50.0, 40.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 9999.0 },
            modifiers: Default::default(),
        });
        tree.layout(SizeProposal::exact(200.0, 150.0));

        let inner_bottom = inner_y.get();
        assert!(inner_bottom > 0.0, "inner should have scrolled down");
        assert!(
            outer_y.get() < 0.01,
            "outer must not move while the inner still absorbs the scroll"
        );

        // Another downward scroll: inner is clamped → the event chains to outer.
        tree.pointer_move(Point::new(50.0, 40.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 100.0 },
            modifiers: Default::default(),
        });
        tree.layout(SizeProposal::exact(200.0, 150.0));

        assert!(
            (inner_y.get() - inner_bottom).abs() < 0.01,
            "inner stays clamped at its bottom"
        );
        assert!(
            outer_y.get() > 0.01,
            "outer scrolled because the inner chained the boundary scroll"
        );
    }

    #[test]
    fn contain_blocks_scroll_chaining() {
        let (mut tree, _inner_y, outer_y) = nested_scroll_fixture(OverscrollBehavior::Contain);

        tree.pointer_move(Point::new(50.0, 40.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 9999.0 },
            modifiers: Default::default(),
        });
        tree.layout(SizeProposal::exact(200.0, 150.0));

        // Inner at bottom + Contain → a further scroll is absorbed, not chained.
        tree.pointer_move(Point::new(50.0, 40.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 100.0 },
            modifiers: Default::default(),
        });
        tree.layout(SizeProposal::exact(200.0, 150.0));

        assert!(
            outer_y.get() < 0.01,
            "Contain must prevent chaining: outer stays put"
        );
    }

    // --- F6: `layout_response` must only pay for the unbounded natural-width
    // measure when the incoming proposal can actually use it ---

    /// A leaf widget that records every `SizeProposal` it's laid out at, in
    /// addition to behaving like [`TallLeaf`] (reports `self.width`/`self.height`
    /// whenever the proposal leaves that axis unspecified).
    #[derive(Debug)]
    struct RecordingLeaf {
        width: f32,
        height: f32,
        log: Rc<std::cell::RefCell<Vec<SizeProposal>>>,
    }

    impl Widget for RecordingLeaf {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> teksilo_core::widget::LayoutResponse {
            self.log.borrow_mut().push(proposal);
            Size::new(
                proposal.width.unwrap_or(self.width),
                proposal.height.unwrap_or(self.height),
            )
            .into()
        }
    }

    #[test]
    fn preferred_height_reports_natural_width_when_parent_proposes_unbounded() {
        // Mirrors `menu_list.rs`: preferred_height set, preferred_size unset,
        // content wider than the old hardcoded 300px fallback.
        let mut tree = WidgetTree::new();
        let content = tree.add(TallLeaf::new(392.0, 500.0));
        let scroll = tree.add(ScrollArea::from_id(content).preferred_height(150.0));

        // Mirrors the popover's own intrinsic-sizing pass: unbounded width.
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });

        let bounds = tree.bounds(scroll);
        assert!(
            (bounds.width - 392.0).abs() < 0.01,
            "should report the content's real natural width, got {}",
            bounds.width
        );
        assert!(
            (bounds.height - 150.0).abs() < 0.01,
            "should still cap the height at preferred_height, got {}",
            bounds.height
        );
    }

    #[test]
    fn bounded_proposal_never_triggers_an_unbounded_content_query() {
        // Plain ScrollArea: neither preferred_size nor preferred_height set.
        let log: Rc<std::cell::RefCell<Vec<SizeProposal>>> =
            Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut tree = WidgetTree::new();
        let content = tree.add(RecordingLeaf {
            width: 900.0,
            height: 500.0,
            log: log.clone(),
        });
        tree.add(ScrollArea::from_id(content));

        // A real parent already bounds the width — the overwhelmingly common case.
        tree.layout(SizeProposal::exact(300.0, 100.0));

        let recorded = log.borrow();
        assert!(!recorded.is_empty(), "content widget was never laid out");
        for proposal in recorded.iter() {
            assert!(
                proposal.width.is_some(),
                "content queried with an unbounded width ({:?}) even though the \
                 incoming proposal was already bounded — the unbounded natural-width \
                 measure must only run when `proposal.width` is `None`",
                proposal
            );
        }
    }

    #[test]
    fn exact_proposal_still_wins_over_natural_width() {
        // No preferred_size / preferred_height: a bounded proposal must still
        // resolve to the proposal's own size, not the content's natural size.
        let mut tree = WidgetTree::new();
        let content = tree.add(TallLeaf::new(900.0, 500.0));
        let scroll = tree.add(ScrollArea::from_id(content));

        tree.layout(SizeProposal::exact(300.0, 100.0));

        let bounds = tree.bounds(scroll);
        assert!(
            (bounds.width - 300.0).abs() < 0.01,
            "exact proposal must win over the content's natural width, got {}",
            bounds.width
        );
        assert!(
            (bounds.height - 100.0).abs() < 0.01,
            "exact proposal must win over the content's natural height, got {}",
            bounds.height
        );
    }

    /// A rigid row wider than the viewport, nested inside a `VStack`, must be
    /// reachable by scrolling horizontally.
    ///
    /// Regression for the cross-axis over-claim asymmetry: `negotiate` used to
    /// end with `self_cross = cross_extent.unwrap_or(self_cross)`, discarding
    /// the larger natural max it had already computed. A `VStack` in a 560 dp
    /// slot holding an 800 dp `HStack` reported 560, so this `ScrollArea` —
    /// which measures content by proposing the viewport width and reading the
    /// size back — concluded "no overflow", showed no horizontal bar, and
    /// `clips_children` swallowed the excess. The 4th cell sat at x=620..820 in
    /// a 600 dp viewport and was unreachable at *any* scroll position.
    ///
    /// The same row placed DIRECTLY under the `ScrollArea` always scrolled;
    /// only the intervening stack broke it, which is what made this so easy to
    /// miss.
    #[test]
    fn cross_axis_overflow_through_a_vstack_is_scrollable() {
        use crate::primitives::{HStack, Padding};

        let mut tree = WidgetTree::new();
        // 4 x 200 dp rigid cells = 800 dp of content in a 600 dp viewport.
        let cells: Vec<_> = (0..4)
            .map(|_| tree.add(TallLeaf::new(200.0, 40.0)))
            .collect();
        let mut row = HStack::new();
        for &c in &cells {
            row = row.add_child(c);
        }
        let row = tree.add(row);
        let col = tree.add(VStack::new().add_child(row));
        let padded = tree.add(Padding::uniform(20.0).child_id(col));
        let _scroll = tree.add(ScrollArea::from_id(padded).smooth_scrolling(false));

        tree.layout(SizeProposal::exact(600.0, 400.0));

        let last = *cells.last().unwrap();
        assert!(
            tree.bounds(last).x > 600.0,
            "precondition: the 4th cell should start beyond the viewport, got x={}",
            tree.bounds(last).x
        );

        // Scroll right far enough to bring the last cell fully into view.
        tree.pointer_move(Point::new(300.0, 40.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 300.0, y: 0.0 },
            modifiers: Default::default(),
        });
        tree.layout(SizeProposal::exact(600.0, 400.0));

        let b = tree.bounds(last);
        assert!(
            b.x >= 0.0 && b.x + b.width <= 600.5,
            "the 4th cell must be reachable by horizontal scrolling; got x={} w={}",
            b.x,
            b.width
        );
    }

    // --- restore_scroll_y: landing a caret-restore offset before the first
    // --- clamp would otherwise destroy it -----------------------------------

    #[test]
    fn restore_scroll_y_lands_on_the_first_measured_layout() {
        // 500px of content in a 100px viewport: max_scroll_y ends up 400.
        let mut tree = WidgetTree::new();
        let sa = ScrollArea::new()
            .child(TallLeaf::new(200.0, 500.0))
            .smooth_scrolling(false)
            .restore_scroll_y(150.0);
        let scroll_y = sa.scroll_y_signal().clone();
        let max_scroll_y = sa.max_scroll_y_signal().clone();
        let _scroll = tree.add(sa);

        // The very first layout pass is also the first at which the content
        // is measured, so the restore must already have landed by the time
        // this call returns; there is no earlier frame to have painted at 0.
        tree.layout(SizeProposal::exact(200.0, 100.0));

        assert_eq!(max_scroll_y.get(), 400.0);
        assert_eq!(
            scroll_y.get(),
            150.0,
            "the restored offset must land on the first laid-out frame"
        );
    }

    #[test]
    fn restore_scroll_y_is_not_re_applied_after_a_later_reflow() {
        let mut tree = WidgetTree::new();
        let sa = ScrollArea::new()
            .child(TallLeaf::new(200.0, 500.0))
            .smooth_scrolling(false)
            .restore_scroll_y(150.0);
        let scroll_y = sa.scroll_y_signal().clone();
        let _scroll = tree.add(sa);

        tree.layout(SizeProposal::exact(200.0, 100.0));
        assert_eq!(scroll_y.get(), 150.0, "precondition: restore landed once");

        // The writer scrolls elsewhere, then something forces a reflow (a
        // window resize, an edit that changes the content's measured size).
        scroll_y.set(70.0);
        tree.layout(SizeProposal::exact(200.0, 120.0));

        assert_eq!(
            scroll_y.get(),
            70.0,
            "a one-shot restore must not re-arm itself on a later reflow"
        );
    }

    #[test]
    fn a_restore_the_content_can_never_hold_does_not_pin_the_reader() {
        // 150px of content in a 100px viewport: `max_scroll_y` is 50 and stays 50,
        // so a pending 200 is never honoured and — before the stand-down below —
        // was re-asserted on every layout pass for the life of the widget.
        //
        // A scroll bar is what makes that fatal rather than merely untidy. It holds
        // a clone of `scroll_y` and calls `set` on it directly, so dragging the
        // thumb never reaches the `on_scroll` handler that stands a restore down:
        // the reader dragged away from the clamped bottom, the next pass put them
        // straight back, and there was no gesture that could win.
        let mut tree = WidgetTree::new();
        let sa = ScrollArea::new()
            .child(TallLeaf::new(200.0, 150.0))
            .smooth_scrolling(false)
            .restore_scroll_y(200.0);
        let scroll_y = sa.scroll_y_signal().clone();
        let _scroll = tree.add(sa);

        tree.layout(SizeProposal::exact(200.0, 100.0));
        assert_eq!(
            scroll_y.get(),
            50.0,
            "precondition: the offset lands clamped to the range that exists"
        );

        // Exactly what `ScrollBar`'s thumb drag does.
        scroll_y.set(0.0);
        tree.layout(SizeProposal::exact(200.0, 100.0));

        assert_eq!(
            scroll_y.get(),
            0.0,
            "a drag away from the clamped landing must stand the restore down, \
             not be undone by the next layout pass"
        );
    }

    #[test]
    fn a_restore_still_waits_out_content_that_is_only_slow_to_measure() {
        // The stand-down must not cost the case the re-apply exists for. A rich
        // text editor reports its `min_lines` height until its own content has been
        // typeset, so the range grows over several passes; the restore has to keep
        // re-asserting through those, and only the *reader* moving may cancel it.
        //
        // The area is laid out three times against a child that grows underneath
        // it, which is what "the content has not finished measuring" looks like
        // from here.
        let mut tree = WidgetTree::new();
        let height = Rc::new(Cell::new(150.0));
        let sa = ScrollArea::new()
            .child(GrowingLeaf::new(200.0, height.clone()))
            .smooth_scrolling(false)
            .restore_scroll_y(200.0);
        let scroll_y = sa.scroll_y_signal().clone();
        let _scroll = tree.add(sa);

        tree.layout(SizeProposal::exact(200.0, 100.0));
        assert_eq!(scroll_y.get(), 50.0, "clamped to the range measured so far");

        height.set(400.0);
        tree.layout(SizeProposal::exact(200.0, 100.0));
        assert_eq!(
            scroll_y.get(),
            200.0,
            "the range grew past the offset, so the offset lands in full"
        );

        // And having landed, it is spent: a later reflow leaves the reader alone.
        scroll_y.set(10.0);
        height.set(900.0);
        tree.layout(SizeProposal::exact(200.0, 100.0));
        assert_eq!(scroll_y.get(), 10.0, "a one-shot does not re-arm");
    }

    #[test]
    fn restore_scroll_y_past_the_range_never_lets_an_observer_see_the_overshoot() {
        // The landing clamps the pending offset itself, which looks redundant beside
        // the `clamp_and_set_scroll` that runs immediately afterwards and would
        // settle on the same final value. It is not redundant, and asserting the
        // final value alone cannot tell the two apart. Writing the raw offset first
        // and correcting it after would publish the overshoot through `scroll_y`, so
        // anything bound to it, a scroll bar's thumb above all, sees a position the
        // content never had. Watch every value the signal takes, not just the last.
        let mut tree = WidgetTree::new();
        let sa = ScrollArea::new()
            .child(TallLeaf::new(200.0, 500.0))
            .smooth_scrolling(false)
            .restore_scroll_y(9999.0);
        let scroll_y = sa.scroll_y_signal().clone();
        let max_scroll_y = sa.max_scroll_y_signal().clone();

        let seen: Rc<std::cell::RefCell<Vec<f32>>> = Rc::new(std::cell::RefCell::new(Vec::new()));
        let recorder = seen.clone();
        let _observer = scroll_y.observe(move |v: &f32| recorder.borrow_mut().push(*v));

        let _scroll = tree.add(sa);
        tree.layout(SizeProposal::exact(200.0, 100.0));

        assert_eq!(
            scroll_y.get(),
            400.0,
            "it must settle at the end of the range"
        );
        assert_eq!(scroll_y.get(), max_scroll_y.get());
        let overshoot: Vec<f32> = seen
            .borrow()
            .iter()
            .copied()
            .filter(|v| *v > max_scroll_y.get())
            .collect();
        assert!(
            overshoot.is_empty(),
            "an observer saw an offset past the end of the content: {overshoot:?}"
        );
    }

    #[test]
    fn restore_scroll_y_waits_for_a_range_long_enough_to_hold_it() {
        // The bug this exists for, found by driving the real app rather than by any
        // headless test: a page holding a long chapter reported a few hundred pixels
        // of content on its first laid-out pass and its true height only later. A
        // restore taken on the first nonzero range landed clamped against the short
        // one, which put the writer back at the top of a chapter they had left the
        // end of, and looked exactly like the restore never happening.
        let height = Rc::new(Cell::new(500.0_f32));
        let mut tree = WidgetTree::new();
        let sa = ScrollArea::new()
            .child(GrowingLeaf::new(200.0, height.clone()))
            .smooth_scrolling(false)
            .restore_scroll_y(11560.0);
        let scroll_y = sa.scroll_y_signal().clone();
        let max_scroll_y = sa.max_scroll_y_signal().clone();
        let _scroll = tree.add(sa);

        tree.layout(SizeProposal::exact(200.0, 100.0));
        assert_eq!(
            max_scroll_y.get(),
            400.0,
            "precondition: a short first pass"
        );
        assert_eq!(
            scroll_y.get(),
            400.0,
            "as far down as the content so far allows, so the page is never at the top"
        );

        height.set(12000.0);
        tree.layout(SizeProposal::exact(200.0, 100.0));
        assert_eq!(
            scroll_y.get(),
            11560.0,
            "once the content is long enough, the offset must land in full"
        );

        // And having landed it, it is spent: growing further must not move the page.
        scroll_y.set(60.0);
        height.set(20000.0);
        tree.layout(SizeProposal::exact(200.0, 100.0));
        assert_eq!(
            scroll_y.get(),
            60.0,
            "a restore already honoured must not re-assert itself on a later reflow"
        );
    }

    #[test]
    fn a_reader_scrolling_stands_down_a_restore_that_has_not_landed() {
        // While the content is still too short to hold the remembered offset, the
        // restore is re-applied on every pass. That must not turn into a fight with
        // someone who has started reading: a real scroll says where they want to be,
        // and outranks a position they left on a previous run.
        let height = Rc::new(Cell::new(500.0_f32));
        let mut tree = WidgetTree::new();
        let sa = ScrollArea::new()
            .child(GrowingLeaf::new(200.0, height.clone()))
            .smooth_scrolling(false)
            .restore_scroll_y(11560.0);
        let scroll_y = sa.scroll_y_signal().clone();
        let _scroll = tree.add(sa);

        tree.layout(SizeProposal::exact(200.0, 100.0));
        assert_eq!(scroll_y.get(), 400.0, "precondition: still pending");

        tree.pointer_move(Point::new(50.0, 40.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 100.0 },
            modifiers: Default::default(),
        });
        let after_reader = scroll_y.get();

        height.set(12000.0);
        tree.layout(SizeProposal::exact(200.0, 100.0));
        assert_eq!(
            scroll_y.get(),
            after_reader,
            "the content growing must not yank a reader who has already scrolled"
        );
    }

    #[test]
    fn without_restore_scroll_y_behaviour_is_unchanged() {
        // Purely additive: an area that never calls `restore_scroll_y` must
        // stay at 0 through layout, exactly as it did before this existed.
        let mut tree = WidgetTree::new();
        let sa = ScrollArea::new()
            .child(TallLeaf::new(200.0, 500.0))
            .smooth_scrolling(false);
        let scroll_y = sa.scroll_y_signal().clone();
        let _scroll = tree.add(sa);

        tree.layout(SizeProposal::exact(200.0, 100.0));
        assert_eq!(scroll_y.get(), 0.0);

        // A later reflow must not conjure an offset out of nowhere either.
        tree.layout(SizeProposal::exact(200.0, 120.0));
        assert_eq!(scroll_y.get(), 0.0);
    }

    #[test]
    fn restore_scroll_y_of_zero_arms_nothing_and_leaves_a_host_write_alone() {
        // Arming `Some(0.0)` and refusing to arm at all reach the same resting
        // position, so asserting the final offset proves nothing about the guard.
        // What separates them is a host that writes the offset itself between
        // construction and the first layout: an armed zero lands on top of that
        // write and wipes it, an unarmed one leaves it standing.
        let mut tree = WidgetTree::new();
        let sa = ScrollArea::new()
            .child(TallLeaf::new(200.0, 500.0))
            .smooth_scrolling(false)
            .restore_scroll_y(0.0);
        let scroll_y = sa.scroll_y_signal().clone();
        let _scroll = tree.add(sa);

        scroll_y.set(120.0);
        tree.layout(SizeProposal::exact(200.0, 100.0));

        assert_eq!(
            scroll_y.get(),
            120.0,
            "restore_scroll_y(0.0) armed a restore and overwrote the host's own offset"
        );
    }

    #[test]
    fn restore_scroll_y_of_zero_disarms_a_previously_armed_offset() {
        // `restore_scroll_y(0.0)` must not merely refuse to arm itself: called after
        // a nonzero call it must clear that earlier value too, or the "no-op" call
        // would silently leave a stale restore pending. Asserted against a host write
        // for the same reason as the test above, so that a still-armed 150.0 and a
        // still-armed 0.0 are both distinguishable from nothing armed at all.
        let mut tree = WidgetTree::new();
        let sa = ScrollArea::new()
            .child(TallLeaf::new(200.0, 500.0))
            .smooth_scrolling(false)
            .restore_scroll_y(150.0)
            .restore_scroll_y(0.0);
        let scroll_y = sa.scroll_y_signal().clone();
        let _scroll = tree.add(sa);

        scroll_y.set(120.0);
        tree.layout(SizeProposal::exact(200.0, 100.0));

        assert_eq!(
            scroll_y.get(),
            120.0,
            "a later restore_scroll_y(0.0) must disarm the earlier pending offset"
        );
    }
}
