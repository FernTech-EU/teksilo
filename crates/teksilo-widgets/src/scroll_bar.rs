// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! ScrollBar — pointer and keyboard affordance for a [`ScrollArea`](crate::scroll_area::ScrollArea).
//!
//! `ScrollBar` reads and writes a shared `Signal<f32>` scroll position and a
//! `Signal<f32>` viewport/content ratio, both supplied by its owning `ScrollArea`.
//! Interaction (thumb drag, track click, keyboard Up/Down/Home/End, hover) is
//! handled here; all painting is delegated to the active [`ScrollBarStyle`] impl so
//! the look is fully theme-overridable.
//!
//! Most applications do not need to construct a `ScrollBar` directly — `ScrollArea`
//! creates and manages the bars automatically. Use this type when building a custom
//! scroll host (e.g. the `RichTextEditor` manages its own bars to avoid the
//! wrap/scrollbar circular dependency).
//!
//! ## Reaching the thumb with a finger
//!
//! The bar is 8–12 dp wide, and it stays that way at every density: growing it
//! would move the content beside it, and a scroll bar is chrome. The thumb is
//! reached instead by the two mechanisms built for exactly this — the node
//! widens for a coarse pointer through [`Widget::hit_outset`], to the 48 dp
//! Android reserves for a scrollbar touch target, and the thumb itself is
//! published through [`Widget::target_regions`] so the target-conformance audit
//! can see a rectangle that is painted inside one leaf node and would otherwise
//! be invisible to it. A precise pointer gets no outset at all: a cursor's
//! hot-spot is exact, and widening its targets steals clicks from the content.
//!
//! Because the outset widens the bar *across* the scroll axis, every decision
//! about whether a press is on the thumb is taken **along the axis only** — a
//! finger 15 dp inboard of an 8 dp bar is beside the thumb, not past it.
//!
//! The minimum thumb length follows the density (24 dp Compact, 44 dp Touch),
//! so a short thumb on a long document is still something a finger can land on.
//!
//! ## Accessibility
//!
//! Hidden from AT via `set_hidden()`. Scroll actions (Up/Down/Left/Right) are
//! advertised on the parent `ScrollView` node, not on the bar, so screen readers
//! navigate the content region directly without stopping on the thumb.
//!
//! ```rust
//! # use teksilo_widgets::scroll_bar::{ScrollBar, ScrollBarOrientation, ScrollBarVariant};
//! # use teksilo_core::signal::Signal;
//! let position = Signal::new(0.0_f32);
//! let max_scroll = Signal::new(500.0_f32);
//! let viewport_ratio = Signal::new(0.4_f32);
//! let _bar = ScrollBar::new(
//!     ScrollBarOrientation::Vertical,
//!     position,
//!     max_scroll,
//!     viewport_ratio,
//! )
//! .thickness(8.0)
//! .variant(ScrollBarVariant::Overlay);
//! ```

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{EdgeInsets, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::color_prop::ColorProp;
use teksilo_core::event::{EventResponse, PointerButton, WidgetEvent};
use teksilo_core::gesture::DragPhase;
use teksilo_core::partition::TargetRegion;
use teksilo_core::signal::Signal;
use teksilo_core::styles::density::dp;
use teksilo_core::styles::{ScrollBarStyle, ScrollBarStyleConfig, SharedScrollBarStyle};
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{RevealPolicy, TargetRole};

use crate::common::range_nav::{self, RangeAxis, RangeKind, RangeMove};

// Re-exports so callers can write `ScrollBar::new(..)` /
// `.visual(ScrollBarVisual::Overlay)` without a deeper import path. The
// `ScrollBarVisual` alias preserves the historical name; new code can
// use `ScrollBarVariant` directly.
pub use teksilo_core::styles::ScrollBarOrientation;
pub use teksilo_core::styles::ScrollBarVariant;
pub use teksilo_core::styles::ScrollBarVariant as ScrollBarVisual;

/// The width a scroll bar's thumb must be reachable across for a finger.
///
/// Android's `ViewConfiguration.MIN_SCROLLBAR_TOUCH_TARGET` — the bar keeps its
/// 8–12 dp paint at every density and reaches this through
/// [`Widget::hit_outset`], which moves nothing and repaints nothing.
pub const SCROLLBAR_COARSE_TARGET: f32 = 48.0;

/// The shipped minimum thumb length, at Compact. Raised to the density's
/// `target_size` (44 dp at Touch) at build time; a
/// [`min_thumb_length`](ScrollBar::min_thumb_length) override wins over both.
pub const SCROLLBAR_MIN_THUMB_LENGTH: f32 = 24.0;

/// Which part of the bar a [`TargetRegion`] describes.
///
/// Reported so an audit — and a router routing a coarse press — can tell the
/// grab affordance from the paging surface around it.
pub const SCROLLBAR_PART_THUMB: u16 = 0;
/// The track either side of the thumb: a tap there pages.
pub const SCROLLBAR_PART_TRACK: u16 = 1;

/// A scroll bar that shares reactive scroll-position state with a [`ScrollArea`](crate::scroll_area::ScrollArea).
///
/// Supports thumb drag, track-click page scroll, and keyboard
/// Up/Down/Left/Right/Home/End navigation. Hidden from AT — see module docs.
pub struct ScrollBar {
    orientation: ScrollBarOrientation,
    /// Scroll position: 0.0 = start, max_scroll = end.
    /// Shared with ScrollArea — both read and write.
    scroll_position: Signal<f32>,
    /// Maximum scroll value (content_size - viewport_size).
    /// Written by the ScrollArea, read by the ScrollBar.
    max_scroll: Signal<f32>,
    /// Viewport / content ratio (0.0..1.0). Determines thumb size.
    /// Written by the ScrollArea, read by the ScrollBar.
    viewport_ratio: Signal<f32>,

    // --- interaction state ---
    /// Whether the pointer is over the scroll bar.
    hovered: Signal<bool>,
    /// Whether the thumb is being dragged.
    dragging: Signal<bool>,
    /// Pointer position at drag start (in scroll bar local coords).
    drag_start_pointer: Rc<Cell<f32>>,
    /// Scroll position at drag start.
    drag_start_scroll: Rc<Cell<f32>>,
    /// Current bounds, cached from last layout for event handling.
    cached_bounds: Rc<Cell<Rect>>,
    /// Layout direction captured at `place_children`, so the thumb geometry —
    /// computed in `build()`'s pointer closures, which have no `PaintContext` —
    /// can mirror a horizontal bar without every caller threading it.
    cached_rtl: Rc<Cell<bool>>,
    /// Body subtree id returned by the active style — kept in
    /// `children()` so layout traverses through it.
    body_id: Option<WidgetId>,

    // --- visual tuning ---
    /// Thickness of the scroll bar (width for vertical, height for horizontal).
    thickness: f32,
    /// Minimum thumb length in pixels, or `None` to follow the density.
    min_thumb_length: Option<f32>,
    /// The density floor [`Self::min_thumb_length`] falls back to, resolved at
    /// the last `build`. Shared with the event closures and read by the
    /// geometry helpers, which run outside a build and have no tokens in scope.
    /// An explicit floor does not go through it — it is known from the moment
    /// it is set, so the geometry is right before the first build too.
    resolved_min_thumb_length: Rc<Cell<f32>>,
    /// Raised from outside — by a `ScrollArea` while a finger's pan is in
    /// flight, and by a density whose `RevealPolicy` is `Always` — to show an
    /// overlay bar that hover alone would keep hidden.
    revealed: Signal<bool>,
    /// Pixels to scroll per keyboard step.
    step_size: f32,
    /// Visual variant: Permanent / Overlay / Thin.
    variant: ScrollBarVariant,
    /// Per-call style override.
    style_override: Option<SharedScrollBarStyle>,
    /// Optional thumb tint. `None` → the style paints from the theme's
    /// `scrollbar_thumb*` tokens; `Some` → tint from this `ColorProp`
    /// (resolved at paint, so a role / `Signal` stays reactive). See
    /// [`Self::thumb_color`].
    thumb_color: Option<ColorProp>,
}

impl std::fmt::Debug for ScrollBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollBar")
            .field("orientation", &self.orientation)
            .field("hovered", &self.hovered.get())
            .field("dragging", &self.dragging.get())
            .field("variant", &self.variant)
            .finish()
    }
}

impl ScrollBar {
    /// Create a new ScrollBar with shared state.
    ///
    /// - `scroll_position`: shared `Signal<f32>` for current scroll offset
    /// - `max_scroll`: shared `Signal<f32>` for maximum scroll offset
    /// - `viewport_ratio`: shared `Signal<f32>` for viewport/content ratio (0.0..1.0)
    pub fn new(
        orientation: ScrollBarOrientation,
        scroll_position: Signal<f32>,
        max_scroll: Signal<f32>,
        viewport_ratio: Signal<f32>,
    ) -> Self {
        // Defaults sourced from `ScrollBarStyle` (Int UI: 8 dp on hover,
        // 4 dp at idle, 24 dp minimum thumb length).
        Self {
            orientation,
            scroll_position,
            max_scroll,
            viewport_ratio,
            hovered: Signal::new(false),
            dragging: Signal::new(false),
            drag_start_pointer: Rc::new(Cell::new(0.0)),
            drag_start_scroll: Rc::new(Cell::new(0.0)),
            cached_bounds: Rc::new(Cell::new(Rect::ZERO)),
            cached_rtl: Rc::new(Cell::new(false)),
            body_id: None,
            thickness: 8.0,
            min_thumb_length: None,
            resolved_min_thumb_length: Rc::new(Cell::new(SCROLLBAR_MIN_THUMB_LENGTH)),
            revealed: Signal::new(false),
            step_size: 40.0,
            variant: ScrollBarVariant::default(),
            style_override: None,
            thumb_color: None,
        }
    }

    /// Set the bar thickness (width for vertical, height for horizontal).
    pub fn thickness(mut self, thickness: f32) -> Self {
        self.thickness = thickness;
        self
    }

    /// Set the minimum thumb length in pixels, overriding the density.
    ///
    /// Left unset the floor is [`SCROLLBAR_MIN_THUMB_LENGTH`] raised to the
    /// density's target size — 24 dp at Compact, 44 dp at Touch — so a short
    /// thumb on a long document stays something a finger can land on.
    pub fn min_thumb_length(mut self, len: f32) -> Self {
        self.min_thumb_length = Some(len);
        self
    }

    /// Show the bar for as long as `revealed` is true, whatever hover says.
    ///
    /// An overlay bar is normally revealed by pointer proximity, which a
    /// contact never produces. `ScrollArea` raises this while a finger's pan is
    /// in flight; a density whose [`RevealPolicy`]
    /// is `Always` seeds it true at build. It only ever adds a reveal — nothing
    /// here can hide a bar that hover has shown.
    pub fn reveal(mut self, revealed: Signal<bool>) -> Self {
        self.revealed = revealed;
        self
    }

    /// Set the scroll step for keyboard navigation.
    pub fn step_size(mut self, step: f32) -> Self {
        self.step_size = step;
        self
    }

    /// Set the visual variant. The active [`ScrollBarStyle`] picks how
    /// to paint each variant; the IntUI default ships Permanent /
    /// Overlay / Thin out of the box.
    pub fn visual(mut self, variant: ScrollBarVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Alias for `visual` using the new variant naming.
    pub fn variant(mut self, variant: ScrollBarVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Override the active [`ScrollBarStyle`] for this widget instance only.
    pub fn style(mut self, style: impl ScrollBarStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Tint the thumb with an explicit colour instead of the theme's
    /// `scrollbar_thumb*` tokens. Accepts anything `impl Into<ColorProp>` —
    /// a `Color`, a theme role (`TextRole`/`SurfaceRole`/…), or a `Signal`;
    /// resolved against the live theme at paint, so roles and signals stay
    /// reactive. The active [`ScrollBarStyle`] derives the idle/hover/pressed
    /// states from this tint. Use when the bar sits on a surface the
    /// surface-relative tokens don't suit — a tooltip's inverse chip, a
    /// branded panel. Mirrors [`Button::text_role`](crate::button::Button::text_role).
    pub fn thumb_color(mut self, color: impl Into<ColorProp>) -> Self {
        self.thumb_color = Some(color.into());
        self
    }

    // --- geometry helpers (kept on the parent because event handlers
    // need them; the style body re-derives the same numbers from cfg).

    /// The total length of the track (along the scroll axis).
    fn track_length(&self) -> f32 {
        let bounds = self.cached_bounds.get();
        match self.orientation {
            ScrollBarOrientation::Vertical => bounds.height,
            ScrollBarOrientation::Horizontal => bounds.width,
        }
    }

    /// The floor the thumb may not be shorter than: the caller's, else the one
    /// the last build resolved from the density.
    fn min_thumb(&self) -> f32 {
        self.min_thumb_length
            .unwrap_or_else(|| self.resolved_min_thumb_length.get())
    }

    /// Computed thumb length based on viewport ratio.
    fn thumb_length(&self) -> f32 {
        let ratio = self.viewport_ratio.get().clamp(0.0, 1.0);
        let track = self.track_length();
        (track * ratio).max(self.min_thumb()).min(track)
    }

    /// Thumb offset from the start of the track.
    fn thumb_offset(&self) -> f32 {
        let max = self.max_scroll.get();
        if max <= 0.0 {
            return 0.0;
        }
        let pos = self.scroll_position.get();
        let ratio = (pos / max).clamp(0.0, 1.0);
        let available = self.track_length() - self.thumb_length();
        ratio * available
    }

    /// The thumb rect in absolute coordinates — the rectangle the active style
    /// actually paints, derived from the same three numbers the painters read.
    fn thumb_rect(&self) -> Rect {
        let bounds = self.cached_bounds.get();
        let offset = self.thumb_offset();
        let thumb_len = self.thumb_length();
        match self.orientation {
            ScrollBarOrientation::Vertical => {
                Rect::new(bounds.x, bounds.y + offset, bounds.width, thumb_len)
            }
            ScrollBarOrientation::Horizontal => {
                Rect::new(bounds.x + offset, bounds.y, thumb_len, bounds.height)
            }
        }
    }
}

impl Widget for ScrollBar {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        // Resolve the active style: per-call override > theme slot >
        // built-in `RecipeScrollBarStyle` default.
        let style: SharedScrollBarStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.scroll_bar.clone())
            .unwrap_or_else(|| {
                Rc::new(crate::styles::RecipeScrollBarStyle::for_tokens(
                    &ctx.theme().input,
                ))
            });

        // The minimum thumb length follows the density unless the caller named
        // one. 24 dp at Compact is exactly today's constant, so `dp` here
        // raises nothing at the density CI runs at — the P20 rule that a
        // dimension may go through `dp` only when its Compact value already
        // clears the conformance floor, which 24 dp does by being it.
        let min_thumb_length = self.min_thumb_length.unwrap_or_else(|| {
            dp(
                SCROLLBAR_MIN_THUMB_LENGTH,
                TargetRole::Target,
                &ctx.theme().input,
            )
        });
        self.resolved_min_thumb_length.set(min_thumb_length);

        // A density that reveals every affordance (Touch) shows the bar without
        // being asked; `ScrollArea` raises the same signal while a pan runs.
        if ctx.theme().input.reveal == RevealPolicy::Always && !self.revealed.get() {
            self.revealed.set(true);
        }

        // Derived `scroll_ratio = scroll_position / max_scroll` (clamped
        // to 0..1). Re-renders the body on every scroll.
        let scroll_ratio = self
            .scroll_position
            .zip(&self.max_scroll)
            .map(|(pos, max)| {
                if *max <= 0.0 {
                    0.0
                } else {
                    (*pos / *max).clamp(0.0, 1.0)
                }
            });
        // `is_idle = max_scroll == 0` — body paints nothing in this case.
        let is_idle = self.max_scroll.map(|m| *m <= 0.0);

        let cfg = ScrollBarStyleConfig {
            scroll_ratio,
            viewport_ratio: self.viewport_ratio.clone(),
            // An external reveal reads as hover to the style: it is the same
            // question ("is this bar being attended to?") asked by a mechanism
            // a contact can answer.
            is_hovered: self.hovered.or(&self.revealed),
            is_dragging: self.dragging.clone(),
            is_idle,
            orientation: self.orientation,
            variant: self.variant,
            min_thumb_length,
            thumb_color: self.thumb_color.clone(),
        };
        let body_id = style.make_body(&cfg, ctx);
        self.body_id = Some(body_id);

        let orientation = self.orientation;
        let scroll_position = self.scroll_position.clone();
        let max_scroll = self.max_scroll.clone();
        let viewport_ratio = self.viewport_ratio.clone();
        let hovered = self.hovered.clone();
        let dragging = self.dragging.clone();
        let drag_start_pointer = self.drag_start_pointer.clone();
        let drag_start_scroll = self.drag_start_scroll.clone();
        let cached_bounds = self.cached_bounds.clone();
        let step_size = self.step_size;

        // A horizontal bar mirrors in a right-to-left window, because
        // `ScrollArea` already anchors its content to the right and grows
        // `scroll_x` leftward. The thumb rect, the drag delta and the
        // track-click direction all key off this one answer, so they cannot
        // disagree. Vertical bars never mirror.
        let mirrored = {
            let cached_rtl = self.cached_rtl.clone();
            move || -> bool {
                matches!(orientation, ScrollBarOrientation::Horizontal) && cached_rtl.get()
            }
        };

        let axis_value = move |point: Point| -> f32 {
            match orientation {
                ScrollBarOrientation::Vertical => point.y,
                ScrollBarOrientation::Horizontal => point.x,
            }
        };

        let set_scroll = {
            let scroll_position = scroll_position.clone();
            let max_scroll = max_scroll.clone();
            move |value: f32| {
                let max = max_scroll.get();
                scroll_position.set(value.clamp(0.0, max));
            }
        };

        let track_length = {
            let cached_bounds = cached_bounds.clone();
            move || -> f32 {
                let bounds = cached_bounds.get();
                match orientation {
                    ScrollBarOrientation::Vertical => bounds.height,
                    ScrollBarOrientation::Horizontal => bounds.width,
                }
            }
        };

        let thumb_length = {
            let viewport_ratio = viewport_ratio.clone();
            let track_length = track_length.clone();
            move || -> f32 {
                let ratio = viewport_ratio.get().clamp(0.0, 1.0);
                let track = track_length();
                (track * ratio).max(min_thumb_length).min(track)
            }
        };

        let thumb_rect = {
            let cached_bounds = cached_bounds.clone();
            let scroll_position = scroll_position.clone();
            let max_scroll = max_scroll.clone();
            let track_length = track_length.clone();
            let thumb_length = thumb_length.clone();
            let mirrored = mirrored.clone();
            move || -> Rect {
                let bounds = cached_bounds.get();
                let max = max_scroll.get();
                let offset = if max <= 0.0 {
                    0.0
                } else {
                    let pos = scroll_position.get();
                    let ratio = (pos / max).clamp(0.0, 1.0);
                    let available = track_length() - thumb_length();
                    ratio * available
                };
                let tl = thumb_length();
                // Widget-local thumb rect (origin at the scrollbar's own
                // top-left): event positions arrive widget-local, so the
                // cross-axis origin is 0, not `bounds.x` / `bounds.y`.
                match orientation {
                    ScrollBarOrientation::Vertical => Rect::new(0.0, offset, bounds.width, tl),
                    ScrollBarOrientation::Horizontal => {
                        let x = if mirrored() {
                            bounds.width - offset - tl
                        } else {
                            offset
                        };
                        Rect::new(x, 0.0, tl, bounds.height)
                    }
                }
            }
        };

        // Is this press on the thumb? Asked **along the scroll axis only**,
        // because `hit_outset` widens the bar across that axis for a coarse
        // pointer: a finger 15 dp inboard of an 8 dp bar is beside the thumb,
        // and treating it as a miss would page the view out from under it.
        let on_thumb = {
            let thumb_rect = thumb_rect.clone();
            move |position: Point| -> bool {
                let tr = thumb_rect();
                let v = axis_value(position);
                match orientation {
                    ScrollBarOrientation::Vertical => v >= tr.y && v <= tr.bottom(),
                    ScrollBarOrientation::Horizontal => v >= tr.x && v <= tr.right(),
                }
            }
        };

        // Scrollbars are pointer affordances. AT scrolls through the parent
        // ScrollView node's ScrollUp/Down/Left/Right actions, not by focusing
        // the scrollbar widget itself.
        let mut handlers = HandlerSet::new().focusable(false);

        // Thumb drag — routed through the typed gesture API. The
        // framework auto-captures the pointer on `DragPhase::Started`
        // and releases it on `DragPhase::Ended`, so thumb drags that
        // leave the widget bounds keep firing.
        //
        // A drag that began off the thumb (e.g. on the track) is
        // deliberately ignored: the `dragging` signal only flips true
        // when the initial press was on the thumb, and track clicks
        // are handled by `on_tap` below.
        {
            let dragging = dragging.clone();
            let drag_start_pointer = drag_start_pointer.clone();
            let drag_start_scroll = drag_start_scroll.clone();
            let scroll_position = scroll_position.clone();
            let max_scroll = max_scroll.clone();
            let set_scroll = set_scroll.clone();
            let on_thumb = on_thumb.clone();
            let track_length = track_length.clone();
            let thumb_length = thumb_length.clone();
            let mirrored = mirrored.clone();
            handlers = handlers.on_drag(move |phase, _ctx| {
                let max = max_scroll.get();
                if max <= 0.0 {
                    return;
                }
                match phase {
                    DragPhase::Started {
                        position,
                        button: PointerButton::Primary,
                        ..
                    } if on_thumb(position) => {
                        dragging.set(true);
                        drag_start_pointer.set(axis_value(position));
                        drag_start_scroll.set(scroll_position.get());
                    }
                    DragPhase::Moved { position, .. } if dragging.get() => {
                        let current = axis_value(position);
                        // Mirrored: dragging right walks *back* towards the
                        // start of the content.
                        let sign = if mirrored() { -1.0 } else { 1.0 };
                        let delta_pixels = (current - drag_start_pointer.get()) * sign;
                        let available = track_length() - thumb_length();
                        if available > 0.0 {
                            let scroll_delta = delta_pixels * max / available;
                            set_scroll(drag_start_scroll.get() + scroll_delta);
                        }
                    }
                    DragPhase::Ended { .. } => {
                        dragging.set(false);
                    }
                    _ => {}
                }
            });
        }

        // Track click — page-scroll toward the click position.
        // The tap recognizer only fires on press+release without
        // movement past the 5 px threshold, so a thumb grab that
        // starts as a click but becomes a drag is handled by the
        // `on_drag` arm above and never reaches here.
        {
            let scroll_position = scroll_position.clone();
            let max_scroll = max_scroll.clone();
            let viewport_ratio = viewport_ratio.clone();
            let set_scroll = set_scroll.clone();
            let thumb_rect = thumb_rect.clone();
            let mirrored = mirrored.clone();
            let on_thumb = on_thumb.clone();
            handlers = handlers.on_tap(move |event, _ctx| {
                let max = max_scroll.get();
                if max <= 0.0 {
                    return;
                }
                let tr = thumb_rect();
                let position = event.position;
                if on_thumb(position) {
                    return;
                }
                let click_axis = axis_value(position);
                let thumb_center = match orientation {
                    ScrollBarOrientation::Vertical => tr.y + tr.height / 2.0,
                    ScrollBarOrientation::Horizontal => tr.x + tr.width / 2.0,
                };
                let ratio = viewport_ratio.get().clamp(0.001, 0.999);
                let viewport_scroll = max * ratio / (1.0 - ratio);
                let current = scroll_position.get();
                // Mirrored: the side of the thumb a click lands on means the
                // opposite page, because the track runs the other way.
                let backwards = (click_axis < thumb_center) != mirrored();
                if backwards {
                    set_scroll(current - viewport_scroll);
                } else {
                    set_scroll(current + viewport_scroll);
                }
            });
        }

        // Hover handler — flips `hovered`. The `active = hovered ||
        // dragging` derivation that drives Fade visibility lives inside
        // the recipe style; no need to thread an explicit signal here.
        {
            let hovered = hovered.clone();
            handlers = handlers.on_hover(move |entered, _ctx| {
                hovered.set(entered);
            });
        }

        // Key handler
        {
            let scroll_position = scroll_position.clone();
            let max_scroll = max_scroll.clone();
            let set_scroll = set_scroll.clone();
            let ratio = self.viewport_ratio.clone();
            let mirrored = mirrored.clone();
            handlers = handlers.on_key(move |event, _ctx| {
                let max = max_scroll.get();
                if max <= 0.0 {
                    return EventResponse::Ignored;
                }
                let WidgetEvent::KeyDown { key, modifiers, .. } = event else {
                    return EventResponse::Ignored;
                };
                let step = step_size;
                // One axis only: a vertical bar must leave the horizontal
                // arrows to the horizontal bar beside it. The direction comes
                // from the same `mirrored` predicate the thumb, the drag and
                // the track click use, so all four agree in a right-to-left
                // window.
                //
                // Reachability, stated plainly: this node is `focusable(false)`
                // and `set_hidden()`, and nothing in the framework focuses it,
                // so today only a programmatic `tree.focus(..)` — a test, or an
                // app that opts in — reaches this handler at all.
                let arrows = match orientation {
                    ScrollBarOrientation::Vertical => RangeAxis::Vertical,
                    ScrollBarOrientation::Horizontal => RangeAxis::Horizontal,
                };
                let Some(mv) =
                    range_nav::range_move(*key, *modifiers, RangeKind::Scalar, arrows, mirrored())
                else {
                    return EventResponse::Ignored;
                };
                // A scroll offset grows *downward*, so `ArrowDown` must add to
                // it even though `range_nav` reports that as a decrease — the
                // value's axis and the screen's disagree on the vertical.
                let horizontal = matches!(orientation, ScrollBarOrientation::Horizontal);
                match mv {
                    RangeMove::Step { increase } => {
                        let d = if range_nav::towards_trailing(increase, horizontal) {
                            step
                        } else {
                            -step
                        };
                        set_scroll(scroll_position.get() + d);
                    }
                    // A page is one viewport of content. The bar already knows
                    // the ratio it draws its thumb from, so `max` (which is
                    // content minus viewport) scaled by `ratio / (1 - ratio)`
                    // recovers the viewport in the same units — and it degrades
                    // to the arrow step when the content barely overflows
                    // rather than jumping nowhere. That the distance is
                    // measured rather than a multiple of the step is exactly
                    // what `RangeMove::Page` leaves to the caller.
                    RangeMove::Page { increase } => {
                        let p = page_step(&ratio, max, step);
                        // Deliberately *not* through `towards_trailing`: the
                        // page keys name a direction in the content, not on
                        // screen, so unlike the arrows they do not follow the
                        // bar's orientation. `PageUp` is one viewport back and
                        // `PageDown` one forward on either axis — which is what
                        // every scroll view binds them to, and what the
                        // vertical bar beside a horizontal one already did.
                        // Reading `increase` geometrically made a horizontal
                        // bar's `PageUp` scroll *forward*.
                        set_scroll(scroll_position.get() + if increase { -p } else { p });
                    }
                    RangeMove::ToMin => set_scroll(0.0),
                    RangeMove::ToMax => set_scroll(max),
                }
                EventResponse::Handled
            });
        }

        // No access-action handler: this node is `set_hidden()`, and assistive
        // technology scrolls through the parent ScrollView's `Scroll*` actions.
        // The `SetValue` arm that used to sit here was never advertised, so its
        // only reachable effect was to answer `Handled` to a `SetValue`
        // bubbling up from a descendant and drop it.

        ctx.apply_self_handlers(handlers);

        vec![body_id]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        match self.orientation {
            ScrollBarOrientation::Vertical => {
                Size::new(self.thickness, proposal.height.unwrap_or(100.0))
            }
            ScrollBarOrientation::Horizontal => {
                Size::new(proposal.width.unwrap_or(100.0), self.thickness)
            }
        }
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        // Cache bounds and direction for event handling: the drag/tap
        // hit-tests recompute the thumb from these in `build()`, with no
        // context of their own, and must mirror a horizontal bar the same way
        // paint does.
        self.cached_bounds.set(bounds);
        self.cached_rtl.set(ctx.is_rtl());
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.body_id.into_iter().collect()
    }

    /// Widen the bar across its scroll axis for a coarse pointer, to the 48 dp
    /// Android reserves for a scrollbar touch target.
    ///
    /// Hit-only: the 8–12 dp paint is untouched at every density, nothing
    /// relayouts, and a Compact build renders byte for byte as it did. Zero for
    /// a mouse and for a pen, both of which are precise enough to land on the
    /// bar as drawn — a pen's tip is where its cursor is, and widening a
    /// precise pointer's targets takes clicks away from the content.
    ///
    /// Only the axis that is too thin grows. Growing the bar *along* the scroll
    /// axis would claim a strip of content above and below it for no benefit:
    /// the track already spans the viewport, and its ends are where the corner
    /// and the other bar live.
    fn hit_outset(
        &self,
        kind: teksilo_tokens::PointerKind,
        _tokens: &teksilo_tokens::InputTokens,
    ) -> EdgeInsets {
        if !matches!(kind, teksilo_tokens::PointerKind::Touch) {
            return EdgeInsets::ZERO;
        }
        let grow = ((SCROLLBAR_COARSE_TARGET - self.thickness) / 2.0).max(0.0);
        match self.orientation {
            ScrollBarOrientation::Vertical => EdgeInsets::symmetric(grow, 0.0),
            ScrollBarOrientation::Horizontal => EdgeInsets::symmetric(0.0, grow),
        }
    }

    /// The thumb, and the track either side of it.
    ///
    /// Both are paint geometry inside one leaf node: the bar's body is a single
    /// private painter widget, so without this the thumb does not exist to
    /// anything outside `paint` — not to the target-conformance audit, and not
    /// to a router that would route a coarse press to the nearest target. The
    /// rectangles come from the same three numbers the painters read, so the
    /// geometry reported and the geometry drawn cannot drift.
    ///
    /// A bar with nothing to scroll paints nothing and reports nothing.
    fn target_regions(&self, bounds: Rect) -> Vec<TargetRegion> {
        if self.max_scroll.get() <= 0.0 {
            return Vec::new();
        }
        // `thumb_rect` reads the bounds cached by the last `place_children`;
        // answer in the caller's frame so a query before the first layout, or
        // after the bar has moved, is still in the space it asked about.
        let cached = self.cached_bounds.get();
        let thumb = self.thumb_rect();
        let thumb = Rect::new(
            bounds.x + (thumb.x - cached.x),
            bounds.y + (thumb.y - cached.y),
            thumb.width,
            thumb.height,
        );
        let mut regions = vec![TargetRegion::grab(thumb, SCROLLBAR_PART_THUMB)];
        // The track pages on a tap, so it is a target in its own right — but
        // only the parts of it the thumb has left over.
        match self.orientation {
            ScrollBarOrientation::Vertical => {
                let before = thumb.y - bounds.y;
                if before > 0.0 {
                    regions.push(TargetRegion::target(
                        Rect::new(bounds.x, bounds.y, bounds.width, before),
                        SCROLLBAR_PART_TRACK,
                    ));
                }
                let after = bounds.bottom() - thumb.bottom();
                if after > 0.0 {
                    regions.push(TargetRegion::target(
                        Rect::new(bounds.x, thumb.bottom(), bounds.width, after),
                        SCROLLBAR_PART_TRACK,
                    ));
                }
            }
            ScrollBarOrientation::Horizontal => {
                let before = thumb.x - bounds.x;
                if before > 0.0 {
                    regions.push(TargetRegion::target(
                        Rect::new(bounds.x, bounds.y, before, bounds.height),
                        SCROLLBAR_PART_TRACK,
                    ));
                }
                let after = bounds.right() - thumb.right();
                if after > 0.0 {
                    regions.push(TargetRegion::target(
                        Rect::new(thumb.right(), bounds.y, after, bounds.height),
                        SCROLLBAR_PART_TRACK,
                    ));
                }
            }
        }
        regions
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Scrollbars are pointer UI. AT uses ScrollUp/Down/Left/Right on the
        // parent ScrollView node — exposing the bar itself adds noise without
        // benefit and creates spurious Tab stops in screen readers.
        builder.set_hidden();
    }
}

/// One viewport of content, in the same units as the scroll position.
///
/// `max` is `content - viewport` and `ratio` is `viewport / content`, so
/// `viewport = max * ratio / (1 - ratio)`. Falls back to the arrow step where
/// that is degenerate (a full or near-full viewport), so `PageDown` always
/// moves rather than silently doing nothing.
fn page_step(ratio: &teksilo_core::signal::Signal<f32>, max: f32, step: f32) -> f32 {
    let r = ratio.get().clamp(0.0, 1.0);
    if r <= 0.0 || r >= 1.0 {
        return step;
    }
    let viewport = max * r / (1.0 - r);
    if viewport.is_finite() && viewport > step {
        viewport
    } else {
        step
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_canvas::SizeProposal;
    use teksilo_core::widget_tree::WidgetTree;

    // A `thumb_color` override must reach the `ScrollBarStyleConfig` the active
    // style sees, so a custom style (or the recipe) can tint the thumb. Mirrors
    // how `Button::text_role` flows into `ButtonStyleConfig`.
    #[test]
    fn thumb_color_override_threads_into_style_config() {
        use std::cell::Cell;
        use std::rc::Rc;
        use teksilo_core::build_context::BuildContext;
        use teksilo_core::styles::{ScrollBarStyle, ScrollBarStyleConfig};

        struct RecordingStyle(Rc<Cell<bool>>);
        impl ScrollBarStyle for RecordingStyle {
            fn make_body(&self, cfg: &ScrollBarStyleConfig, ctx: &mut BuildContext) -> WidgetId {
                self.0.set(cfg.thumb_color.is_some());
                ctx.add(crate::primitives::Spacer::new())
            }
        }

        let saw_override = Rc::new(Cell::new(false));
        let mut tree = WidgetTree::new();
        let bar = ScrollBar::new(
            ScrollBarOrientation::Vertical,
            Signal::new(0.0),
            Signal::new(500.0),
            Signal::new(0.5),
        )
        .style(RecordingStyle(saw_override.clone()))
        .thumb_color(teksilo_tokens::TextRole::TooltipText);
        tree.add(bar);
        tree.layout(SizeProposal::exact(20.0, 200.0));
        assert!(
            saw_override.get(),
            "ScrollBar::thumb_color must thread into ScrollBarStyleConfig::thumb_color"
        );
    }

    // ── Keyboard ───────────────────────────────────────────────────
    //
    // These reach the handler through a *programmatic* focus, which does not
    // consult `focusable`. That is deliberate and worth stating: the bar is
    // `focusable(false)` and `set_hidden()`, and nothing in the framework
    // focuses it, so today no user keystroke arrives here at all. The handler
    // is routed through the shared chord table anyway, so it is correct if an
    // application ever opts a bar into the tab order.

    fn focused_bar(ratio: f32, max: f32) -> (WidgetTree, Signal<f32>, WidgetId) {
        let position = Signal::new(0.0_f32);
        let mut tree = WidgetTree::new();
        let id = tree.add(ScrollBar::new(
            ScrollBarOrientation::Vertical,
            position.clone(),
            Signal::new(max),
            Signal::new(ratio),
        ));
        tree.layout(SizeProposal::exact(20.0, 200.0));
        tree.focus(id);
        (tree, position, id)
    }

    #[test]
    fn arrows_step_the_position() {
        use teksilo_core::event::{Key, Modifiers};
        let (mut tree, position, id) = focused_bar(0.5, 500.0);

        // Stated first, because it is the interesting half: if focus does not
        // even land on a `focusable(false)` node, no keystroke can reach the
        // handler and the rest of this module's keyboard is unreachable.
        assert_eq!(
            tree.focused(),
            Some(id),
            "programmatic focus must land for any of these to mean anything"
        );

        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        let after = position.get();
        assert!(after > 0.0, "ArrowDown scrolls down: {after}");

        tree.press_key(Key::ArrowUp, Modifiers::NONE);
        assert!(position.get() < after, "ArrowUp scrolls back");
    }

    #[test]
    fn a_vertical_bar_ignores_the_horizontal_arrows() {
        // The horizontal bar beside it owns them; a bar that answered both
        // would fight its sibling.
        use teksilo_core::event::{Key, Modifiers};
        let (mut tree, position, _) = focused_bar(0.5, 500.0);

        for key in [Key::ArrowLeft, Key::ArrowRight] {
            tree.press_key(key, Modifiers::NONE);
            assert_eq!(position.get(), 0.0, "{key:?} is not a vertical bar's");
        }
    }

    #[test]
    fn home_and_end_reach_the_ends() {
        use teksilo_core::event::{Key, Modifiers};
        let (mut tree, position, _) = focused_bar(0.5, 500.0);

        tree.press_key(Key::End, Modifiers::NONE);
        assert_eq!(position.get(), 500.0);
        tree.press_key(Key::Home, Modifiers::NONE);
        assert_eq!(position.get(), 0.0);
    }

    #[test]
    fn page_keys_move_one_measured_viewport() {
        // The page distance is *measured*, not a multiple of the arrow step:
        // with `ratio = 0.2` and `max = 400`, the viewport is
        // `400 * 0.2 / 0.8 = 100`.
        use teksilo_core::event::{Key, Modifiers};
        let (mut tree, position, _) = focused_bar(0.2, 400.0);

        tree.press_key(Key::PageDown, Modifiers::NONE);
        assert!(
            (position.get() - 100.0).abs() < 0.01,
            "one viewport is 100, got {}",
            position.get()
        );
        tree.press_key(Key::PageUp, Modifiers::NONE);
        assert!(position.get().abs() < 0.01);
    }

    #[test]
    fn the_page_keys_read_the_content_not_the_screen() {
        // `PageUp` is one viewport *back* and `PageDown` one forward on either
        // axis. The horizontal bar used to run them through the same
        // `towards_trailing` mapping the arrows use, which reads `increase`
        // geometrically — so its `PageUp` scrolled forward and its `PageDown`
        // back, the opposite of the vertical bar sitting beside it.
        use teksilo_core::event::{Key, Modifiers};

        for orientation in [
            ScrollBarOrientation::Vertical,
            ScrollBarOrientation::Horizontal,
        ] {
            let position = Signal::new(200.0_f32);
            let mut tree = WidgetTree::new();
            let id = tree.add(ScrollBar::new(
                orientation,
                position.clone(),
                Signal::new(400.0),
                Signal::new(0.2),
            ));
            tree.layout(match orientation {
                ScrollBarOrientation::Vertical => SizeProposal::exact(20.0, 200.0),
                ScrollBarOrientation::Horizontal => SizeProposal::exact(200.0, 20.0),
            });
            tree.focus(id);

            tree.press_key(Key::PageDown, Modifiers::NONE);
            assert!(
                position.get() > 200.0,
                "{orientation:?}: PageDown moves forward through the content, got {}",
                position.get()
            );
            let forward = position.get();
            tree.press_key(Key::PageUp, Modifiers::NONE);
            assert!(
                position.get() < forward,
                "{orientation:?}: PageUp moves back, got {}",
                position.get()
            );
        }
    }

    #[test]
    fn an_accelerator_chord_does_not_scroll() {
        // Behaviour change: modifiers used to be ignored, so `Ctrl+End` jumped
        // to the bottom and swallowed the chord.
        use teksilo_core::event::{Key, Modifiers};
        let (mut tree, position, _) = focused_bar(0.5, 500.0);

        for (key, mods) in [
            (Key::End, Modifiers::CTRL),
            (Key::PageDown, Modifiers::ALT),
            (Key::ArrowDown, Modifiers::SUPER),
        ] {
            tree.press_key(key, mods);
            assert_eq!(
                position.get(),
                0.0,
                "{key:?} with {mods:?} must fall through"
            );
        }
    }

    fn make_scrollbar() -> (ScrollBar, Signal<f32>, Signal<f32>, Signal<f32>) {
        let position = Signal::new(0.0_f32);
        let max_scroll = Signal::new(500.0_f32);
        let viewport_ratio = Signal::new(0.5_f32); // viewport is half of content

        let bar = ScrollBar::new(
            ScrollBarOrientation::Vertical,
            position.clone(),
            max_scroll.clone(),
            viewport_ratio.clone(),
        );
        (bar, position, max_scroll, viewport_ratio)
    }

    #[test]
    fn vertical_scrollbar_size() {
        let (bar, ..) = make_scrollbar();
        let mut tree = WidgetTree::new();
        let id = tree.add(bar);
        tree.layout(SizeProposal {
            width: None,
            height: Some(400.0),
        });

        let bounds = tree.bounds(id);
        // Vertical: width = thickness (8), height = proposed (400)
        assert!((bounds.width - 8.0).abs() < 0.01);
        assert!((bounds.height - 400.0).abs() < 0.01);
    }

    #[test]
    fn horizontal_scrollbar_size() {
        let position = Signal::new(0.0_f32);
        let max_scroll = Signal::new(500.0_f32);
        let viewport_ratio = Signal::new(0.5_f32);

        let bar = ScrollBar::new(
            ScrollBarOrientation::Horizontal,
            position,
            max_scroll,
            viewport_ratio,
        );
        let mut tree = WidgetTree::new();
        let id = tree.add(bar);
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: None,
        });

        let bounds = tree.bounds(id);
        // Horizontal: width = proposed (400), height = thickness (8)
        assert!((bounds.width - 400.0).abs() < 0.01);
        assert!((bounds.height - 8.0).abs() < 0.01);
    }

    #[test]
    fn scrollbar_thumb_drag_updates_position() {
        let (bar, position, _max, _ratio) = make_scrollbar();
        let mut tree = WidgetTree::new();
        let _id = tree.add(bar);
        tree.layout(SizeProposal::exact(12.0, 400.0));

        // Render once to cache bounds
        tree.render();

        // Initial position is 0
        assert!((position.get() - 0.0).abs() < 0.01);

        // Pointer down on the thumb (which starts at top)
        tree.pointer_move(Point::new(6.0, 10.0));
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(6.0, 10.0),
            button: PointerButton::Primary,
            modifiers: teksilo_core::event::Modifiers::NONE,
        });

        // Drag 100px down: track is 400px, thumb is 200px (50% ratio),
        // so available travel = 200px, 100px drag = 50% of travel = 250 scroll.
        // DragRecognizer needs one move to cross the 5px threshold and emit
        // DragStarted (which carries the *down* position, so the thumb-vs-track
        // check latches on), then subsequent moves emit DragMoved with a delta
        // from the initial press.
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(6.0, 20.0),
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(6.0, 110.0),
        });

        let pos = position.get();
        assert!(pos > 200.0, "Expected scroll > 200, got {}", pos);
        assert!(pos < 300.0, "Expected scroll < 300, got {}", pos);
    }

    #[test]
    fn scrollbar_clamps_to_range() {
        let (bar, position, max_scroll, ..) = make_scrollbar();
        let mut tree = WidgetTree::new();
        tree.add(bar);
        tree.layout(SizeProposal::exact(12.0, 400.0));
        tree.render();

        // Repeatedly click far below the thumb to page-scroll forward
        // until we hit the maximum (500). Each page scroll adds 250,
        // so after 3 clicks the position should be clamped at 500.
        for _ in 0..5 {
            tree.pointer_move(Point::new(6.0, 390.0));
            tree.dispatch_event(WidgetEvent::PointerDown {
                position: Point::new(6.0, 390.0),
                button: PointerButton::Primary,
                modifiers: teksilo_core::event::Modifiers::NONE,
            });
            // Release so next click isn't a drag
            tree.dispatch_event(WidgetEvent::PointerUp {
                position: Point::new(6.0, 390.0),
                button: PointerButton::Primary,
                modifiers: teksilo_core::event::Modifiers::NONE,
            });
        }

        let pos = position.get();
        let max = max_scroll.get();
        assert!(
            (pos - max).abs() < 0.01,
            "Expected pos to be clamped at max={}, got {}",
            max,
            pos,
        );
    }

    #[test]
    fn scrollbar_nothing_to_scroll() {
        let position = Signal::new(0.0_f32);
        let max_scroll = Signal::new(0.0_f32); // content fits in viewport
        let viewport_ratio = Signal::new(1.0_f32);

        let bar = ScrollBar::new(
            ScrollBarOrientation::Vertical,
            position,
            max_scroll,
            viewport_ratio,
        );
        let mut tree = WidgetTree::new();
        tree.add(bar);
        tree.layout(SizeProposal::exact(12.0, 400.0));

        let frame = tree.render();
        // When max_scroll is 0, the body's `is_idle` gate suppresses
        // every paint, so no shapes get queued.
        assert!(
            frame.shapes.is_empty(),
            "Expected no rendering when nothing to scroll"
        );
    }

    #[test]
    fn scrollbar_is_hidden_from_at() {
        // ScrollBar is a pointer affordance. AT scrolls through the parent
        // ScrollView's actions, not by navigating the bar directly.
        let (bar, position, _max_scroll, _ratio) = make_scrollbar();
        position.set(100.0);

        let mut tree = WidgetTree::new();
        let id = tree.add(bar);
        tree.layout(SizeProposal::exact(12.0, 400.0));

        let info = tree.accessibility_node(id);
        assert!(info.is_hidden(), "ScrollBar must be hidden from AT");
    }

    #[test]
    fn a_horizontal_bar_mirrors_its_track_in_rtl() {
        // `ScrollArea` already anchors its content to the right in a
        // right-to-left window and grows `scroll_x` leftward, so the start of
        // the content is at the *right*. A thumb pinned to the geometric left
        // therefore sat at the far end of the track while the content showed
        // its beginning, and a track click paged the wrong way.
        use teksilo_core::event::Modifiers;

        let position = Signal::new(0.0_f32);
        let mut tree = WidgetTree::new();
        tree.set_layout_direction(teksilo_core::environment::LayoutDirection::RightToLeft);
        let _id = tree.add(ScrollBar::new(
            ScrollBarOrientation::Horizontal,
            position.clone(),
            Signal::new(500.0),
            Signal::new(0.5),
        ));
        tree.layout(SizeProposal::exact(400.0, 12.0));
        tree.render();

        // At scroll 0 the thumb is flush *right*, so the empty track is on the
        // left — clicking there pages forward, the mirror of the LTR case.
        let p = Point::new(40.0, 6.0);
        tree.pointer_move(p);
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: p,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: p,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        assert!(
            position.get() > 0.0,
            "a click left of a right-anchored thumb pages forward, got {}",
            position.get()
        );
    }

    #[test]
    fn a_horizontal_bar_mirrors_its_painted_thumb_in_rtl() {
        // The style paints the thumb; the widget hit-tests and drags it. Both
        // have to mirror or the two disagree — the painted thumb sat at the
        // geometric left while the grab region was on the right, so the thumb
        // jumped the moment it was touched. The direction is read at paint
        // time, so a locale change needs no rebuild.
        let position = Signal::new(0.0_f32);
        let mut tree = WidgetTree::new();
        tree.set_layout_direction(teksilo_core::environment::LayoutDirection::RightToLeft);
        tree.add(ScrollBar::new(
            ScrollBarOrientation::Horizontal,
            position.clone(),
            Signal::new(500.0),
            Signal::new(0.5),
        ));
        tree.layout(SizeProposal::exact(400.0, 12.0));
        let frame = tree.render();

        // Half a 400 px track is a 200 px thumb; at scroll 0 it is flush with
        // the *start* of the content, which is the right edge here.
        let thumb = frame
            .shapes
            .iter()
            .find(|q| (q.screen[2] - 200.0).abs() < 1.0)
            .expect("the bar paints a 200 px thumb");
        assert!(
            (thumb.screen[0] - 200.0).abs() < 1.0,
            "the thumb must be flush right at scroll 0 under RTL, got x={}",
            thumb.screen[0]
        );
    }

    #[test]
    fn a_horizontal_bar_mirrors_its_arrows_in_rtl() {
        // The arrows key off the same predicate as the thumb and the track
        // click, so all four agree rather than each deciding for itself.
        use teksilo_core::event::{Key, Modifiers};

        let position = Signal::new(200.0_f32);
        let mut tree = WidgetTree::new();
        tree.set_layout_direction(teksilo_core::environment::LayoutDirection::RightToLeft);
        let id = tree.add(ScrollBar::new(
            ScrollBarOrientation::Horizontal,
            position.clone(),
            Signal::new(500.0),
            Signal::new(0.5),
        ));
        tree.layout(SizeProposal::exact(400.0, 12.0));
        tree.focus(id);

        tree.press_key(Key::ArrowLeft, Modifiers::NONE);
        assert!(
            position.get() > 200.0,
            "under RTL the leftward arrow travels forward through the content"
        );
    }

    #[test]
    fn a_vertical_bar_ignores_the_layout_direction() {
        use teksilo_core::event::{Key, Modifiers};

        let position = Signal::new(200.0_f32);
        let mut tree = WidgetTree::new();
        tree.set_layout_direction(teksilo_core::environment::LayoutDirection::RightToLeft);
        let id = tree.add(ScrollBar::new(
            ScrollBarOrientation::Vertical,
            position.clone(),
            Signal::new(500.0),
            Signal::new(0.5),
        ));
        tree.layout(SizeProposal::exact(12.0, 400.0));
        tree.focus(id);

        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert!(
            position.get() > 200.0,
            "there is no leading/trailing on the vertical axis to mirror"
        );
    }

    #[test]
    fn track_click_pages_forward() {
        let (bar, position, ..) = make_scrollbar();
        let mut tree = WidgetTree::new();
        let _id = tree.add(bar);
        tree.layout(SizeProposal::exact(12.0, 400.0));
        tree.render();

        // Click on the track below the thumb (thumb starts at top, ~200px tall).
        // Track clicks are routed through `on_tap`, which requires a full
        // press+release sequence without the pointer crossing the drag
        // threshold.
        tree.pointer_move(Point::new(6.0, 350.0));
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(6.0, 350.0),
            button: PointerButton::Primary,
            modifiers: teksilo_core::event::Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(6.0, 350.0),
            button: PointerButton::Primary,
            modifiers: teksilo_core::event::Modifiers::NONE,
        });

        let pos = position.get();
        assert!(
            pos > 0.0,
            "Expected positive scroll after track click, got {}",
            pos
        );
    }

    #[test]
    fn scrollbar_drag_inside_scroll_area_updates_position() {
        // Regression: reproduces the real-app case where the ScrollBar
        // is a child of a ScrollArea (overlay mode), which wraps a tall
        // content widget. Before the V2 migration this worked through
        // `on_pointer_event`; the drag must keep working through the
        // typed `on_drag` + auto-capture path.
        use crate::primitives::MinSize;
        use crate::scroll_area::{ScrollArea, ScrollBarMode};
        use teksilo_canvas::Point;
        use teksilo_core::event::{Modifiers, PointerButton};

        let mut tree = WidgetTree::new();
        // Content is twice as tall as the ScrollArea viewport → v scrollbar
        // is needed with viewport_ratio = 0.5.
        let content = MinSize::new(400.0, 800.0);
        let root = tree.add(
            ScrollArea::new()
                .child(content)
                .scroll_bar_style(ScrollBarMode::Permanent),
        );
        tree.layout(SizeProposal::exact(400.0, 400.0));
        tree.render();

        // Find the vertical scrollbar child (second child of ScrollArea:
        // content is first, v-scrollbar second).
        let sb_id = tree.children(root)[1];
        let sb_bounds = tree.bounds(sb_id);
        assert!(
            sb_bounds.width > 0.0,
            "scrollbar should have non-zero width"
        );
        assert!(
            sb_bounds.height > 0.0,
            "scrollbar should have non-zero height"
        );

        // Press in the middle of the thumb (thumb spans y=sb_bounds.y..+half).
        let thumb_cx = sb_bounds.x + sb_bounds.width / 2.0;
        let thumb_cy = sb_bounds.y + sb_bounds.height / 4.0;
        tree.pointer_move(Point::new(thumb_cx, thumb_cy));
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(thumb_cx, thumb_cy),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        // Cross the drag threshold…
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(thumb_cx, thumb_cy + 10.0),
        });
        // …and then actually drag down.
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(thumb_cx, thumb_cy + 100.0),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(thumb_cx, thumb_cy + 100.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        // Apply the scroll-triggered relayout so the content's cached
        // bounds reflect the new scroll offset (the real event loop does
        // this automatically every frame).
        tree.layout(SizeProposal::exact(400.0, 400.0));

        // The scroll position should have advanced by a substantial amount
        // (a 100-px drag on a 400-px track with 50 % viewport ratio moves
        // the content ~200 px).
        let final_scroll = tree.hit_test(Point::new(1.0, 1.0)); // dummy, just keep borrow checker quiet
        let _ = final_scroll;
        // We can't read scroll_y directly from the public API; assert the
        // *bounds* of the content child moved in the ScrollArea's layout
        // rect — after layout the content's origin.y is `-scroll_y`.
        let content_bounds = tree.bounds(tree.children(root)[0]);
        assert!(
            content_bounds.y < -1.0,
            "content should have scrolled up (y < 0); got y={}",
            content_bounds.y
        );
    }

    #[test]
    fn drag_release_outside_does_not_stick() {
        // Regression test: dragging the thumb and releasing outside the
        // scrollbar must not leave `dragging` stuck to true. This requires
        // pointer capture so that PointerUp reaches the scrollbar even when
        // the pointer is outside its bounds.
        let (bar, position, ..) = make_scrollbar();
        let mut tree = WidgetTree::new();
        let _id = tree.add(bar);
        tree.layout(SizeProposal::exact(12.0, 400.0));
        tree.render();

        // Start drag on the thumb
        tree.pointer_move(Point::new(6.0, 10.0));
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(6.0, 10.0),
            button: PointerButton::Primary,
            modifiers: teksilo_core::event::Modifiers::NONE,
        });

        // Move far outside the scrollbar bounds
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(200.0, 300.0),
        });

        // Release outside
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(200.0, 300.0),
            button: PointerButton::Primary,
            modifiers: teksilo_core::event::Modifiers::NONE,
        });

        // Now hover the scrollbar again — should NOT continue dragging
        let pos_before = position.get();
        tree.pointer_move(Point::new(6.0, 50.0));
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(6.0, 50.0),
        });

        let pos_after = position.get();
        assert!(
            (pos_after - pos_before).abs() < 0.01,
            "Hovering after release should not move scroll: before={}, after={}",
            pos_before,
            pos_after,
        );
    }
}

/// Reaching the thumb with a finger: the hit mechanisms, the density floor,
/// and the axis-only decisions the widened bar forces.
#[cfg(test)]
mod touch_tests {
    use super::*;
    use teksilo_canvas::SizeProposal;
    use teksilo_core::event::{Modifiers, WidgetEvent};
    use teksilo_core::widget::{LayoutContext, Widget};
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_tokens::{InputTokens, PenKind, PointerKind, TargetDensity};

    /// A 12 dp vertical bar over 400 dp of track, content twice the viewport —
    /// so the thumb is half the track and starts at the top.
    fn bar() -> (ScrollBar, Signal<f32>, Signal<f32>, Signal<f32>) {
        let position = Signal::new(0.0_f32);
        let max_scroll = Signal::new(500.0_f32);
        let viewport_ratio = Signal::new(0.5_f32);
        let bar = ScrollBar::new(
            ScrollBarOrientation::Vertical,
            position.clone(),
            max_scroll.clone(),
            viewport_ratio.clone(),
        )
        .thickness(12.0);
        (bar, position, max_scroll, viewport_ratio)
    }

    fn mounted() -> (WidgetTree, WidgetId, Signal<f32>) {
        let (bar, position, ..) = bar();
        let mut tree = WidgetTree::new();
        let id = tree.add(bar);
        tree.layout(SizeProposal::exact(12.0, 400.0));
        tree.render();
        (tree, id, position)
    }

    /// The bar's own reported regions, primed the way a layout pass primes
    /// them. There is no tree-level accessor for `target_regions` yet — the
    /// conformance walker that will grow one is a later package — so the widget
    /// is asked directly, off the tree, which is also what makes the reported
    /// geometry checkable against the arithmetic the painters use rather than
    /// against itself.
    fn regions_of(bar: &ScrollBar, bounds: Rect) -> Vec<TargetRegion> {
        let theme = teksilo_core::presets::intui::light();
        let ctx = LayoutContext::for_testing(&theme);
        bar.place_children(
            bounds,
            SizeProposal::exact(bounds.width, bounds.height),
            &mut [],
            &ctx,
        );
        bar.target_regions(bounds)
    }

    // -- target_regions ---------------------------------------------------

    /// The reported thumb is the rectangle the style paints: half a 400 dp
    /// track at a 0.5 viewport ratio, at the top while the scroll is at zero.
    /// Nothing outside this widget can otherwise see it — the whole bar is one
    /// leaf node whose body paints the thumb inside its own `paint`.
    #[test]
    fn target_regions_report_the_thumb_at_its_paint_rect() {
        let (bar, position, ..) = bar();
        let bounds = Rect::new(0.0, 0.0, 12.0, 400.0);

        let regions = regions_of(&bar, bounds);
        let thumb = regions
            .iter()
            .find(|r| r.part == SCROLLBAR_PART_THUMB)
            .expect("the thumb is reported");
        assert_eq!(thumb.role, TargetRole::Grab);
        assert!((thumb.rect.y - bounds.y).abs() < 0.01, "{:?}", thumb.rect);
        assert!((thumb.rect.height - 200.0).abs() < 0.01, "{:?}", thumb.rect);
        assert!((thumb.rect.width - 12.0).abs() < 0.01);

        // Scrolled to the end, the thumb is at the end of the track.
        position.set(500.0);
        let regions = regions_of(&bar, bounds);
        let thumb = regions
            .iter()
            .find(|r| r.part == SCROLLBAR_PART_THUMB)
            .expect("the thumb is reported");
        assert!(
            (thumb.rect.bottom() - bounds.bottom()).abs() < 0.01,
            "{:?}",
            thumb.rect
        );
    }

    /// The regions are answered in the frame the caller asked about, so an
    /// audit that asks about a bar sitting at an offset gets rectangles there
    /// rather than at the origin the last layout happened to use.
    #[test]
    fn target_regions_answer_in_the_frame_they_were_asked_about() {
        let (bar, ..) = bar();
        let moved = Rect::new(180.0, 40.0, 12.0, 400.0);
        let thumb = regions_of(&bar, moved)
            .into_iter()
            .find(|r| r.part == SCROLLBAR_PART_THUMB)
            .expect("the thumb");
        assert!((thumb.rect.x - 180.0).abs() < 0.01, "{:?}", thumb.rect);
        assert!((thumb.rect.y - 40.0).abs() < 0.01, "{:?}", thumb.rect);
    }

    /// The track either side of the thumb is a target too — a tap there pages —
    /// and it is exactly the part of the bar the thumb has left over.
    #[test]
    fn target_regions_report_the_paging_track_around_the_thumb() {
        let (bar, position, ..) = bar();
        position.set(250.0);
        let regions = regions_of(&bar, Rect::new(0.0, 0.0, 12.0, 400.0));
        let thumb = regions
            .iter()
            .find(|r| r.part == SCROLLBAR_PART_THUMB)
            .expect("the thumb");
        let track: Vec<_> = regions
            .iter()
            .filter(|r| r.part == SCROLLBAR_PART_TRACK)
            .collect();
        assert_eq!(track.len(), 2, "one strip above the thumb and one below");
        assert!((track[0].rect.bottom() - thumb.rect.y).abs() < 0.01);
        assert!((track[1].rect.y - thumb.rect.bottom()).abs() < 0.01);
    }

    /// A bar with nothing to scroll paints nothing, so it reports nothing.
    #[test]
    fn a_bar_with_nothing_to_scroll_reports_no_targets() {
        let bar = ScrollBar::new(
            ScrollBarOrientation::Vertical,
            Signal::new(0.0),
            Signal::new(0.0),
            Signal::new(1.0),
        );
        assert!(regions_of(&bar, Rect::new(0.0, 0.0, 12.0, 400.0)).is_empty());
    }

    // -- hit_outset -------------------------------------------------------

    /// A finger reaches the bar across 48 dp; the paint stays 12 dp, and the
    /// growth is on the cross axis only.
    #[test]
    fn a_finger_reaches_a_48_dp_bar_over_a_12_dp_paint() {
        let (bar, ..) = bar();
        let tokens = InputTokens::for_density(TargetDensity::Compact);
        let outset = bar.hit_outset(PointerKind::Touch, &tokens);
        assert_eq!(outset.leading, 18.0);
        assert_eq!(outset.trailing, 18.0);
        assert_eq!(outset.top, 0.0, "the track already spans the viewport");
        assert_eq!(outset.bottom, 0.0);
        assert_eq!(
            12.0 + outset.leading + outset.trailing,
            SCROLLBAR_COARSE_TARGET
        );
    }

    /// A precise pointer gets nothing — its hot-spot is exact, and widening it
    /// would take clicks from the content beside the bar. Same at every
    /// density: this is a property of the device, not of the ladder.
    #[test]
    fn a_precise_pointer_gets_no_outset_at_any_density() {
        let (bar, ..) = bar();
        for density in [
            TargetDensity::Compact,
            TargetDensity::Comfortable,
            TargetDensity::Touch,
        ] {
            let tokens = InputTokens::for_density(density);
            for kind in [PointerKind::Mouse, PointerKind::Pen(PenKind::Pen)] {
                assert_eq!(
                    bar.hit_outset(kind, &tokens),
                    EdgeInsets::ZERO,
                    "{kind:?} at {density:?}"
                );
            }
        }
    }

    /// A horizontal bar grows the other way.
    #[test]
    fn a_horizontal_bar_grows_vertically() {
        let bar = ScrollBar::new(
            ScrollBarOrientation::Horizontal,
            Signal::new(0.0),
            Signal::new(500.0),
            Signal::new(0.5),
        )
        .thickness(8.0);
        let tokens = InputTokens::for_density(TargetDensity::Compact);
        let outset = bar.hit_outset(PointerKind::Touch, &tokens);
        assert_eq!(outset.top, 20.0);
        assert_eq!(outset.bottom, 20.0);
        assert_eq!(outset.leading, 0.0);
        assert_eq!(outset.trailing, 0.0);
    }

    // -- the minimum thumb ------------------------------------------------

    /// The floor follows the density — 24 dp at Compact, which is exactly the
    /// value this widget has always shipped, and 44 dp at Touch.
    /// What the widget resolved reaches the style, which is the number the
    /// painters actually size the thumb from.
    fn floor_seen_by_the_style(density: TargetDensity, explicit: Option<f32>) -> f32 {
        use std::cell::Cell;
        use std::rc::Rc;
        use teksilo_core::build_context::BuildContext;
        use teksilo_core::styles::{ScrollBarStyle, ScrollBarStyleConfig};

        struct Recording(Rc<Cell<f32>>);
        impl ScrollBarStyle for Recording {
            fn make_body(&self, cfg: &ScrollBarStyleConfig, ctx: &mut BuildContext) -> WidgetId {
                self.0.set(cfg.min_thumb_length);
                ctx.add(crate::primitives::Spacer::new())
            }
        }

        let seen = Rc::new(Cell::new(f32::NAN));
        let mut bar = ScrollBar::new(
            ScrollBarOrientation::Vertical,
            Signal::new(0.0),
            Signal::new(500.0),
            Signal::new(0.02),
        )
        .style(Recording(seen.clone()));
        if let Some(explicit) = explicit {
            bar = bar.min_thumb_length(explicit);
        }
        let mut tree = WidgetTree::new();
        tree.set_input_density(density);
        tree.add(bar);
        tree.layout(SizeProposal::exact(12.0, 400.0));
        seen.get()
    }

    /// The floor follows the density — 24 dp at Compact, which is exactly the
    /// value this widget has always shipped, and 44 dp at Touch.
    #[test]
    fn the_minimum_thumb_follows_the_density() {
        assert_eq!(floor_seen_by_the_style(TargetDensity::Compact, None), 24.0);
        assert_eq!(
            floor_seen_by_the_style(TargetDensity::Comfortable, None),
            32.0
        );
        assert_eq!(floor_seen_by_the_style(TargetDensity::Touch, None), 44.0);
    }

    /// An explicit floor wins over the density.
    #[test]
    fn an_explicit_minimum_thumb_wins_over_the_density() {
        assert_eq!(
            floor_seen_by_the_style(TargetDensity::Touch, Some(60.0)),
            60.0
        );
    }

    /// …and the floor the widget resolved is the one its own geometry uses, so
    /// the thumb an audit reads and the thumb a press lands on are the same
    /// rectangle at every density.
    #[test]
    fn the_resolved_floor_reaches_the_reported_thumb() {
        let bar = ScrollBar::new(
            ScrollBarOrientation::Vertical,
            Signal::new(0.0),
            Signal::new(500.0),
            Signal::new(0.02),
        )
        .min_thumb_length(44.0);
        let thumb = regions_of(&bar, Rect::new(0.0, 0.0, 12.0, 400.0))
            .into_iter()
            .find(|r| r.part == SCROLLBAR_PART_THUMB)
            .expect("the thumb");
        assert_eq!(thumb.rect.height, 44.0);
    }

    // -- grabbing and paging ----------------------------------------------

    /// A press inside the thumb's own paint rectangle grabs it. The bar is
    /// widened for a finger, so this is the case the axis-only test has to keep
    /// answering the same way it always did.
    #[test]
    fn the_thumb_is_grabbable_at_its_paint_rect() {
        let thumb = regions_of(&bar().0, Rect::new(0.0, 0.0, 12.0, 400.0))
            .into_iter()
            .find(|r| r.part == SCROLLBAR_PART_THUMB)
            .expect("the thumb");
        let (mut tree, _id, position) = mounted();
        let grab = Point::new(thumb.rect.center().x, thumb.rect.center().y);

        tree.pointer_move(grab);
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: grab,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(grab.x, grab.y + 10.0),
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(grab.x, grab.y + 100.0),
        });
        assert!(
            position.get() > 200.0,
            "the drag moved the thumb: {}",
            position.get()
        );
    }

    /// A finger 13 dp inboard of a 12 dp bar reaches it through the outset —
    /// and, being level with the thumb, grabs the thumb rather than paging the
    /// track. Deciding thumb-versus-track on the full rectangle would have
    /// paged the view out from under the finger that was reaching for the grab,
    /// which is why that decision is taken along the scroll axis alone.
    #[test]
    fn a_finger_reaching_through_the_outset_grabs_the_thumb() {
        use teksilo_core::pointer::{
            BackendDeviceKey, EventTime, PointerIdAllocator, PointerInfo, PointerPhase,
            PointerSample,
        };

        let (bar, position, ..) = bar();
        let mut tree = WidgetTree::new();
        let bar_id = tree.add(bar);
        // The bar at the trailing edge of a 200 dp row, with a spacer taking
        // the rest — an overlay bar's arrangement, and the one where the outset
        // has content beside it to reach across.
        tree.add(
            crate::primitives::HStack::new()
                .child(crate::primitives::Spacer::new())
                .add_child(bar_id),
        );
        tree.layout(SizeProposal::exact(200.0, 400.0));
        tree.render();
        let bounds = tree.bounds(bar_id);
        assert!((bounds.width - 12.0).abs() < 0.01, "the paint is unchanged");

        let alloc = PointerIdAllocator::global();
        let device = BackendDeviceKey::new(0x5B47);
        let finger = alloc.begin(device, 1);
        alloc.end(device, 1);
        let sample = |phase: PointerPhase, at: Point| PointerSample {
            pointer: PointerInfo::touch(finger, EventTime::ZERO),
            phase,
            position: at,
            button: None,
            modifiers: Modifiers::NONE,
            coalesced: Vec::new(),
        };

        // 13 dp inboard of the bar: off its paint, inside its 18 dp outset,
        // and level with a thumb that spans the top half of the track.
        let grab = Point::new(bounds.x - 13.0, 100.0);
        assert_eq!(
            tree.hit_test_for(grab, &PointerInfo::touch(finger, EventTime::ZERO)),
            Some(bar_id),
            "the outset put the finger on the bar"
        );
        assert_ne!(
            tree.hit_test(grab),
            Some(bar_id),
            "…and a mouse at the same point lands on the content beside it"
        );
        tree.dispatch_pointer(sample(PointerPhase::Down, grab));
        tree.dispatch_pointer(sample(
            PointerPhase::Move,
            Point::new(grab.x, grab.y + 30.0),
        ));
        tree.dispatch_pointer(sample(
            PointerPhase::Move,
            Point::new(grab.x, grab.y + 100.0),
        ));
        assert!(
            position.get() > 200.0,
            "the finger dragged the thumb rather than paging: {}",
            position.get()
        );
    }

    /// A tap on the track past the thumb pages one viewport toward it.
    #[test]
    fn a_track_tap_pages_toward_the_tap() {
        let (mut tree, _id, position) = mounted();
        let tap = Point::new(6.0, 390.0);
        tree.pointer_move(tap);
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: tap,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: tap,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        // max = 500 at a 0.5 ratio, so one viewport is 500 × 0.5 / 0.5 = 500,
        // clamped to the end.
        assert_eq!(position.get(), 500.0);

        // …and a tap above the thumb pages back.
        let tap = Point::new(6.0, 10.0);
        tree.pointer_move(tap);
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: tap,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: tap,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(position.get(), 0.0);
    }

    // -- reveal -----------------------------------------------------------

    /// An external reveal reads as hover to the style, which is how an overlay
    /// bar becomes visible under a gesture that writes no hover.
    #[test]
    fn an_external_reveal_reads_as_hover_to_the_style() {
        use std::cell::Cell;
        use std::rc::Rc;
        use teksilo_core::build_context::BuildContext;
        use teksilo_core::styles::{ScrollBarStyle, ScrollBarStyleConfig};

        #[derive(Clone)]
        struct Recording(Rc<Cell<bool>>);
        impl ScrollBarStyle for Recording {
            fn make_body(&self, cfg: &ScrollBarStyleConfig, ctx: &mut BuildContext) -> WidgetId {
                self.0.set(cfg.is_hovered.get());
                ctx.add(crate::primitives::Spacer::new())
            }
        }

        let hovered = Rc::new(Cell::new(false));
        let revealed = Signal::new(false);
        let bar = ScrollBar::new(
            ScrollBarOrientation::Vertical,
            Signal::new(0.0),
            Signal::new(500.0),
            Signal::new(0.5),
        )
        .reveal(revealed.clone())
        .style(Recording(hovered.clone()));
        let mut tree = WidgetTree::new();
        tree.add(bar);
        tree.layout(SizeProposal::exact(12.0, 400.0));
        assert!(!hovered.get(), "nothing has revealed it yet");

        revealed.set(true);
        tree.layout(SizeProposal::exact(12.0, 400.0));
        let bar = ScrollBar::new(
            ScrollBarOrientation::Vertical,
            Signal::new(0.0),
            Signal::new(500.0),
            Signal::new(0.5),
        )
        .reveal(revealed.clone())
        .style(Recording(hovered.clone()));
        let mut tree = WidgetTree::new();
        tree.add(bar);
        tree.layout(SizeProposal::exact(12.0, 400.0));
        assert!(hovered.get(), "a raised reveal shows the bar");
    }

    /// A density that reveals every affordance seeds the reveal itself, so a
    /// touch build's overlay bar is visible without anyone raising it.
    #[test]
    fn a_touch_density_reveals_the_bar_at_rest() {
        let revealed = Signal::new(false);
        let bar = ScrollBar::new(
            ScrollBarOrientation::Vertical,
            Signal::new(0.0),
            Signal::new(500.0),
            Signal::new(0.5),
        )
        .reveal(revealed.clone());
        let mut tree = WidgetTree::new();
        tree.set_input_density(TargetDensity::Touch);
        tree.add(bar);
        tree.layout(SizeProposal::exact(12.0, 400.0));
        assert!(revealed.get(), "RevealPolicy::Always shows it at rest");
    }

    /// …and a Compact build does not, which is the invariant every density
    /// change in this programme has to keep.
    #[test]
    fn a_compact_density_leaves_the_bar_at_rest() {
        let revealed = Signal::new(false);
        let bar = ScrollBar::new(
            ScrollBarOrientation::Vertical,
            Signal::new(0.0),
            Signal::new(500.0),
            Signal::new(0.5),
        )
        .reveal(revealed.clone());
        let mut tree = WidgetTree::new();
        tree.add(bar);
        tree.layout(SizeProposal::exact(12.0, 400.0));
        assert!(!revealed.get());
    }
}
