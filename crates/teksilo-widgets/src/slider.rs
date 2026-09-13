// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Slider — a draggable value selector bound to a `Signal<f32>`.
//!
//! The widget owns all input handling: pointer drag (click-to-jump and
//! thumb-drag), the keyboard, and the `Increment` / `Decrement` /
//! `SetValue` accessibility actions. All visual chrome is delegated to a
//! [`SliderStyle`] implementation; the
//! IntUI default ships out of the box and is also the theme-wide slot
//! override target (`theme.style_slots.slider`).
//!
//! ## Keyboard
//!
//! - `ArrowRight` / `ArrowUp` and `ArrowLeft` / `ArrowDown` — one
//!   [`step`](Slider::step), defaulting to 1 % of the range.
//! - `PageUp` / `PageDown` — one [`page_step`](Slider::page_step),
//!   defaulting to ten times the step and so to 10 % of the range. That
//!   is `QAbstractSlider::pageStep`, `GtkScale`'s page increment, and
//!   what `<input type=range>` gives in both WebKit and Blink.
//! - `Home` / `End` — the minimum and the maximum.
//! - A chord holding `Ctrl`, `Alt` or `Super` is not the slider's and
//!   falls through to the application; `Shift` does not change the step.
//!
//! The chord table is shared with every other bounded-scalar control; see
//! `docs/range-keyboard.md`.
//!
//! ## Accessibility
//!
//! Exposes `Role::Slider` with numeric value, min, max, step, the page
//! distance as `numeric_value_jump`, and orientation. Screen readers
//! announce the current value on every change. `SetValue` accepts a
//! number or a numeric string and snaps it to `step`, so an assistive
//! technology's write lands on the same grid a drag does — AT-SPI's
//! `Value.SetCurrentValue` is how Orca sets a slider, and macOS gates
//! `setAccessibilityValue:` settability on the action being advertised.
//! The focus ring follows the `:focus-visible` heuristic — visible after
//! keyboard interaction, invisible after a pointer tap.
//!
//! ```rust
//! # use teksilo_core::signal::Signal;
//! # use teksilo_widgets::Slider;
//! let volume = Signal::new(0.5_f32);
//! let _w = Slider::new(volume, 0.0, 1.0).step(0.05);
//! ```
//!
//! ## Touch and pen
//!
//! A slider is a **continuous manipulator**: the value it produces *is* the
//! press position, so a finger that lands on it adjusts it — even inside a
//! scrolling form, and from the first movement rather than after a long-press
//! timer. Two separate things deliver that. The press *capture* the drag takes
//! makes the slider the innermost member of the pointer's sequence, which is
//! what stops an enclosing scroller winning the gesture. `touch_action(NONE)`
//! is the declaration on top: it forbids every default touch behaviour on the
//! hit path, which in practice means a **two-contact pinch** started on the
//! slider never reaches the surface under it. `docs/touch-and-pen.md` §7.3.
//!
//! [`teksilo_core::widget::Widget::target_regions`]
//! reports what the style painted inside the slider's one node: the whole node
//! as the press surface, and the knob as the grab affordance, sized through the
//! resolved style's density-aware `thumb_diameter_for`. Nothing else in the
//! tree can see the knob — it is drawn on the same canvas as the track — so
//! this is the only way a conformance audit or a coarse-press router learns it
//! is there.
//!
//! **Right-to-left.** A horizontal slider's minimum sits at the *leading* edge,
//! which is the right-hand one in an RTL UI, so both the painted fill and the
//! position→value map mirror. They were previously mirrored in neither, so an
//! RTL slider's knob moved away from the finger dragging it.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::event::{EventResponse, PointerButton, WidgetEvent};
use teksilo_core::focus::FocusOrigin;
use teksilo_core::gesture::DragPhase;
use teksilo_core::pointer::touch_action::TouchAction;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::styles::{
    SharedSliderStyle, SliderOrientation, SliderStyle, SliderStyleConfig, SliderVariant,
};
use teksilo_core::widget::{CursorIcon, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::Orientation;

use crate::common::range_nav::{self, RangeAxis, RangeKind, RangeMove};

// Re-export the variant enum at module top so callers can write
// `Slider::new(...).variant(SliderVariant::Discrete)` without a deeper
// import path.
pub use teksilo_core::styles::SliderVariant as SliderVariantExport;
use teksilo_i18n::LocalizedString;

/// [`Widget::target_regions`] part id for the whole press surface — the track
/// plus everything either side of it, which is what a tap or a drag acts on.
pub const SLIDER_PART_BODY: u16 = 0;
/// [`Widget::target_regions`] part id for the knob.
pub const SLIDER_PART_THUMB: u16 = 1;

/// A draggable value selector bound to a `Signal<f32>` in a continuous
/// or discrete range. Visual chrome is fully delegated to a
/// [`SliderStyle`] implementation.
pub struct Slider {
    value: Signal<f32>,
    min: f32,
    max: f32,
    step: Option<f32>,
    page_step: Option<f32>,
    orientation: Orientation,
    /// Enabled state, static or reactive; forwarded to the arena at
    /// build time.
    enabled: Prop<bool>,
    /// Accessible name, announced by screen readers as the control's label.
    label: Option<LocalizedString>,
    variant: SliderVariant,
    tick_count: Option<u32>,
    style_override: Option<SharedSliderStyle>,
    hovered: Signal<bool>,
    dragging: Signal<bool>,
    /// Raw keyboard/pointer focus (any modality). The keyboard-only focus
    /// ring is derived live from this × the input-modality signal in
    /// `build()` (`:focus-visible`).
    focused: Signal<bool>,
    cached_bounds: Rc<Cell<Rect>>,
    /// Reading direction, captured in `place_children` (the one hook with a
    /// `LayoutContext`) so both the position→value map, which only ever sees an
    /// `EventContext`, and `target_regions`, which sees neither, agree with
    /// what the style painted.
    cached_rtl: Rc<Cell<bool>>,
    /// The knob diameter the resolved style reported at build time, kept so
    /// `target_regions` can place the thumb exactly where the body painted it.
    thumb_diameter: Cell<f32>,
    body_id: Option<WidgetId>,
    /// Optional plain tooltip text shown after a hover delay. Mutually exclusive
    /// with the rich / composite slots — every setter clears the other two so
    /// the last call wins.
    tooltip_text: Option<LocalizedString>,
    /// Optional rich tooltip source (registry key or inline content).
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Optional composite tooltip body (arbitrary widget tree).
    composite_tooltip_content: Option<Box<dyn Widget>>,
}

impl Slider {
    /// Create a horizontal slider bound to `value` with the given inclusive
    /// range. Use [`orientation`](Self::orientation) to switch to vertical.
    pub fn new(value: Signal<f32>, min: f32, max: f32) -> Self {
        Self {
            value,
            min,
            max,
            step: None,
            page_step: None,
            orientation: Orientation::Horizontal,
            enabled: Prop::Static(true),
            label: None,
            variant: SliderVariant::default(),
            tick_count: None,
            style_override: None,
            hovered: Signal::new(false),
            dragging: Signal::new(false),
            focused: Signal::new(false),
            cached_bounds: Rc::new(Cell::new(Rect::ZERO)),
            cached_rtl: Rc::new(Cell::new(false)),
            thumb_diameter: Cell::new(0.0),
            body_id: None,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
        }
    }

    /// Set the discrete step size for keyboard arrows and accessibility
    /// Increment/Decrement actions. When unset, defaults to 1 % of the
    /// range.
    pub fn step(mut self, step: f32) -> Self {
        self.step = Some(step);
        self
    }

    /// Set the step size for `PageUp` / `PageDown`. When unset, ten times the
    /// effective [`step`](Self::step) — so with the default step of 1 % of the
    /// range a page is 10 % of it, which is what `QAbstractSlider::pageStep`,
    /// `GtkScale`'s page increment and `<input type=range>` in both WebKit and
    /// Blink all give, and the same `10 x` rule
    /// [`SpinBox::page_step`](crate::SpinBox::page_step) uses.
    pub fn page_step(mut self, page_step: f32) -> Self {
        self.page_step = Some(page_step);
        self
    }

    /// The effective arrow step: the configured one, else 1 % of the range.
    ///
    /// One definition for the key handler, the AccessKit
    /// `Increment`/`Decrement` path and the published `numeric_value_step`,
    /// which had drifted into two copies of the same fallback.
    fn effective_step(&self) -> f32 {
        self.step.unwrap_or((self.max - self.min) * 0.01)
    }

    /// The effective `PageUp` / `PageDown` distance.
    fn effective_page_step(&self) -> f32 {
        self.page_step.unwrap_or(self.effective_step() * 10.0)
    }

    /// Set the slider orientation (`Horizontal` by default). Vertical
    /// sliders map Up/Down arrow keys to increase/decrease.
    pub fn orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Set the enabled state, statically or reactively. Forwarded to
    /// the arena at build time via
    /// `ctx.enabled_when(slider_id, self.enabled.clone())`.
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
        self
    }

    /// Pick a Tier-1 design-language variant
    /// ([`SliderVariant::Continuous`] / `Discrete` / `Range`). The
    /// active [`SliderStyle`] decides what to do with the hint —
    /// IntUI's default impl paints ticks for `Discrete` and ignores
    /// `Range` (the widget itself doesn't yet wire dual-thumb
    /// behaviour).
    pub fn variant(mut self, variant: SliderVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Configure the tick count for a `Discrete` slider. The
    /// IntUI default paints `n` evenly spaced tick marks above the
    /// track (or to the leading side for vertical orientation).
    pub fn tick_count(mut self, count: u32) -> Self {
        self.tick_count = Some(count);
        self
    }

    /// Override the active [`SliderStyle`] for this widget instance
    /// only.
    pub fn style(mut self, style: impl SliderStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Set an accessible name for the slider, announced by screen readers.
    /// ARIA requires sliders to have a label; when none is set here the
    /// caller is responsible for labelling via a wrapping element.
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        self.label = Some(ls);
        self
    }

    /// Attach a plain single-line tooltip shown after a hover delay.
    /// Mutually exclusive with [`rich_tooltip`](Self::rich_tooltip),
    /// [`rich_tooltip_content`](Self::rich_tooltip_content), and
    /// [`composite_tooltip`](Self::composite_tooltip) — the last setter
    /// wins and clears the others.
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip driven by a registry key. The registry
    /// entry supplies title, body markup, optional shortcut chip and
    /// cascade links. Mutually exclusive with the other tooltip setters.
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip from an inline [`TooltipContent`](crate::tooltip::TooltipContent)
    /// value, bypassing the registry lookup. Mutually exclusive with the
    /// other tooltip setters.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip whose body is an arbitrary widget tree.
    /// Uses the heavier `tooltip_delay_heavy` delay. Mutually exclusive
    /// with the other tooltip setters.
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }
}

impl std::fmt::Debug for Slider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Slider")
            .field("min", &self.min)
            .field("max", &self.max)
            .field("enabled", &self.enabled.get())
            .field("variant", &self.variant)
            .finish()
    }
}

impl Widget for Slider {
    fn build(
        &mut self,
        ctx: &mut teksilo_core::build_context::BuildContext,
    ) -> Vec<teksilo_core::widget_id::WidgetId> {
        let self_id = ctx.self_id();
        // Forward the enabled state into the arena; see IconButton.
        ctx.enabled_when(self_id, self.enabled.clone());
        let effective_enabled = ctx.effective_enabled_signal(self_id);

        // Resolve the active style: per-call override > theme slot >
        // built-in `RecipeSliderStyle` default.
        let style: SharedSliderStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.slider.clone())
            .unwrap_or_else(|| {
                Rc::new(crate::styles::RecipeSliderStyle::for_tokens(
                    &ctx.theme().input,
                ))
            });

        // Derived `value_normalized` signal — re-renders the body
        // whenever the user-visible value changes.
        let min = self.min;
        let max = self.max;
        let value_normalized = self.value.map(move |v| {
            let range = max - min;
            if range <= 0.0 {
                0.0
            } else {
                ((*v - min) / range).clamp(0.0, 1.0)
            }
        });

        let orientation = match self.orientation {
            Orientation::Horizontal => SliderOrientation::Horizontal,
            Orientation::Vertical => SliderOrientation::Vertical,
        };

        let cfg = SliderStyleConfig {
            value_normalized,
            is_hovered: self.hovered.clone(),
            is_dragging: self.dragging.clone(),
            is_disabled: effective_enabled.map(|on| !*on),
            // `:focus-visible`: derive the keyboard/pointer origin live from
            // the input-modality signal (true after a key event, false after
            // pointer-down) rather than snapshotting hover at focus time, so
            // the focus ring follows the *current* modality.
            focus_origin: self.focused.zip(&ctx.focus_visible()).map(|(f, v)| {
                if !*f {
                    None
                } else if *v {
                    Some(FocusOrigin::Keyboard)
                } else {
                    Some(FocusOrigin::POINTER)
                }
            }),
            orientation,
            tick_count: self.tick_count,
            variant: self.variant,
        };
        let body_id = style.make_body(&cfg, ctx);
        self.body_id = Some(body_id);

        // Capture the thumb radius at build time. The event handlers
        // need it for value computation, but they only receive
        // `EventContext` and can't reach the theme at event time.
        // Query the *resolved* style so a custom `SliderStyle` with a
        // different thumb size keeps drag hit-testing aligned, instead of
        // baking in the recipe's design constant. Through the density-aware
        // overload, so a style that does size its knob by density is asked the
        // question that lets it answer; the default forwards to the plain
        // `thumb_diameter` and the painted 14 dp knob is unchanged.
        let thumb_radius = style.thumb_diameter_for(&cfg, &ctx.theme().input) * 0.5;
        self.thumb_diameter.set(thumb_radius * 2.0);

        let value = self.value.clone();
        let single_step = self.effective_step();
        let page_step = self.effective_page_step();
        let snap_step = self.effective_step();
        let orientation = self.orientation;
        let hovered = self.hovered.clone();
        let dragging = self.dragging.clone();
        let focused = self.focused.clone();
        let cached_bounds = self.cached_bounds.clone();

        let adjust_by_step = {
            let value = value.clone();
            move |positive: bool, page: bool| {
                let s = if page { page_step } else { single_step };
                let current = value.get();
                let new_val = if positive { current + s } else { current - s };
                value.set(new_val.clamp(min, max));
            }
        };

        // Snap-and-clamp writer. `set_value_from_position` decides *where* the
        // pointer is; this decides what a value means once you have one — so an
        // assistive technology's `SetValue` lands on the same grid a drag does
        // instead of between two ticks.
        //
        // The grid is [`effective_step`](Self::effective_step), not the
        // configured `step`: that is the distance the arrows move, the distance
        // `Increment` / `Decrement` move, and the one published as
        // `numeric_value_step`, so snapping to anything else would let a write
        // land between two values every other path can reach. A slider that
        // configures no step gets the same 1 %-of-range grid its arrows already
        // walk — a hundred positions, which is what `<input type=range>` gives
        // a stepless range too.
        let set_value_snapped = {
            let value = value.clone();
            move |v: f32| {
                let mut val = v;
                if snap_step > 0.0 {
                    val = ((val - min) / snap_step).round() * snap_step + min;
                }
                value.set(val.clamp(min, max));
            }
        };

        let set_value_from_position = {
            let set_value_snapped = set_value_snapped.clone();
            let cached_bounds = cached_bounds.clone();
            move |x: f32, y: f32, rtl: bool| {
                let bounds = cached_bounds.get();
                let pos = match orientation {
                    Orientation::Horizontal => x,
                    Orientation::Vertical => y,
                };
                let usable = match orientation {
                    Orientation::Horizontal => bounds.width,
                    Orientation::Vertical => bounds.height,
                } - thumb_radius * 2.0;
                if usable <= 0.0 {
                    return;
                }
                // `pos` arrives widget-local (origin at the slider's own
                // top-left), so the track starts at `thumb_radius`, not at
                // `bounds.x` / `bounds.y`.
                let mut t = ((pos - thumb_radius) / usable).clamp(0.0, 1.0);
                // The minimum sits at the leading edge, which is the right one
                // in a right-to-left window, so a horizontal slider reads its
                // pointer position from the other end.
                //
                // A vertical slider mirrors unconditionally instead: its
                // minimum is at the **bottom** and its maximum at the top —
                // Qt, GTK4, the Win32 trackbar and `<input type=range>` all
                // agree, and it is what makes `ArrowUp` raise the thumb rather
                // than lower it. `y` grows downward, so the raw ratio is the
                // value's complement. The layout direction does not reach this
                // axis: there is no leading/trailing on the vertical.
                match orientation {
                    Orientation::Horizontal => {
                        if rtl {
                            t = 1.0 - t;
                        }
                    }
                    Orientation::Vertical => t = 1.0 - t,
                }
                set_value_snapped(min + t * (max - min));
            }
        };

        // Framework gates events on `arena.is_enabled(self_id)`, so
        // no per-handler enabled snapshot guards anymore.
        let mut handlers = HandlerSet::new()
            .focusable(true)
            .cursor(CursorIcon::Pointer)
            // A continuous manipulator: the value it produces IS the press
            // position, so no default touch behaviour may be run on it. `NONE`
            // freezes the whole hit path's touch action at the press
            // (`docs/touch-and-pen.md` §7.3, A6). What that observably forbids
            // — and what `two_contacts_on_a_slider_pinch_nothing_underneath`
            // holds it to — is a **two-contact pinch** started on the slider
            // reaching the surface underneath. Keeping an enclosing scroller
            // off the press is the drag's capture, not this.
            .touch_action(TouchAction::NONE);

        // Thumb drag — routed through the typed gesture API.
        {
            let dragging = dragging.clone();
            let set_value = set_value_from_position.clone();
            handlers = handlers.on_drag(move |phase, ctx| match phase {
                DragPhase::Started {
                    position,
                    button: PointerButton::Primary,
                    ..
                } => {
                    dragging.set(true);
                    set_value(position.x, position.y, ctx.is_rtl());
                }
                DragPhase::Moved { position, .. } if dragging.get() => {
                    set_value(position.x, position.y, ctx.is_rtl());
                }
                DragPhase::Ended { .. } => {
                    dragging.set(false);
                }
                _ => {}
            });
        }

        // Track click — jump the value to the click position.
        {
            let set_value = set_value_from_position.clone();
            handlers = handlers.on_tap(move |event, ctx| {
                set_value(event.position.x, event.position.y, ctx.is_rtl());
            });
        }

        // Hover handler
        {
            let hovered = hovered.clone();
            handlers = handlers.on_hover(move |entered, _ctx| {
                hovered.set(entered);
            });
        }

        // Key handler. The chord table is shared with every other bounded
        // scalar (`common::range_nav`); what stays here is the arithmetic.
        {
            let adjust = adjust_by_step.clone();
            let value = value.clone();
            handlers = handlers.on_key(move |event, ctx| {
                let WidgetEvent::KeyDown { key, modifiers, .. } = event else {
                    return EventResponse::Ignored;
                };
                // The paint, the pointer mapping and the arrows all mirror
                // against this same answer, read at event time so a locale
                // flip needs no rebuild. Mirroring any one of the three alone
                // would leave the `Left` key and a leftward drag moving the
                // thumb in opposite directions.
                let Some(mv) = range_nav::range_move(
                    *key,
                    *modifiers,
                    RangeKind::Scalar,
                    RangeAxis::Both,
                    ctx.is_rtl(),
                ) else {
                    return EventResponse::Ignored;
                };
                match mv {
                    RangeMove::Step { increase } => adjust(increase, false),
                    RangeMove::Page { increase } => adjust(increase, true),
                    RangeMove::ToMin => value.set(min),
                    RangeMove::ToMax => value.set(max),
                }
                EventResponse::Handled
            });
        }

        // Focus handler. Track raw focus only; the keyboard/pointer
        // distinction is derived live from the input-modality signal in
        // `build()` (`:focus-visible`), so clicking to focus then pressing a
        // key reveals the ring.
        {
            let focused = focused.clone();
            handlers = handlers.on_focus(move |gained, _ctx| {
                focused.set(gained);
            });
        }

        // Access action handler. `SetValue` is the assistive technology's
        // direct value write — AT-SPI's `Value.SetCurrentValue`, which is
        // Orca's slider value entry, and macOS's `setAccessibilityValue:` on
        // the AXSlider. AT-SPI publishes the `Value` interface off
        // `numeric_value` alone, so that call already reached this widget and
        // was dropped on the floor; macOS instead gates AXValue settability on
        // `supports_action(SetValue, ..)`, which is why `accessibility()` has
        // to advertise it. It is the payload handler because `ActionData` is
        // where the value rides, and Increment and Decrement sit beside it so
        // the whole action set reads as one match rather than being split
        // across two slots.
        {
            let adjust = adjust_by_step.clone();
            let set_snapped = set_value_snapped.clone();
            handlers = handlers.on_access_action_request(move |action, _node, data, _ctx| {
                use teksilo_core::accesskit::{Action, ActionData};
                match (action, data) {
                    (Action::Increment, _) => {
                        adjust(true, false);
                        EventResponse::Handled
                    }
                    (Action::Decrement, _) => {
                        adjust(false, false);
                        EventResponse::Handled
                    }
                    // A non-finite value would clamp to a bound rather than be
                    // refused, so it is declined outright: writing a number the
                    // caller did not ask for is worse than reporting failure.
                    (Action::SetValue, Some(ActionData::NumericValue(v))) if v.is_finite() => {
                        set_snapped(v as f32);
                        EventResponse::Handled
                    }
                    // The string shape arrives from an `NSString` write and
                    // from Teksilo's own automation `set_value` tool, which
                    // sends only strings.
                    (Action::SetValue, Some(ActionData::Value(s))) => {
                        match s.trim().parse::<f32>() {
                            Ok(v) if v.is_finite() => {
                                set_snapped(v);
                                EventResponse::Handled
                            }
                            _ => EventResponse::Ignored,
                        }
                    }
                    _ => EventResponse::Ignored,
                }
            });
        }

        ctx.apply_self_handlers(handlers);

        // Tooltip attachment — at most one branch fires (the setters are
        // mutually exclusive). Anchor on `body_id`, the primary visible root.
        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, body_id, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, body_id, source, delay);
        } else if let Some(text) = self.tooltip_text.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_plain_tooltip(ctx, body_id, text, delay);
        }

        vec![body_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.body_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        // Cache bounds for event handling (needed before paint).
        self.cached_bounds.set(bounds);
        self.cached_rtl.set(ctx.is_rtl());
        if let Some(child) = children.first_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.body_id.into_iter().collect()
    }

    /// What a slider paints inside its one node: a track and a knob.
    ///
    /// The node itself is the *target* — a press anywhere on it jumps the value
    /// and a drag from anywhere moves it — so it is reported whole, and it is
    /// what the conformance audit measures (the shipped body is at least 24 dp
    /// across, `MIN_CROSS_SIZE` in `styles/recipe_slider_style.rs`). The knob is
    /// reported as a **grab** because that is the affordance a user aims at,
    /// and because nothing else in the tree can see that it exists: it is drawn
    /// on the same canvas as the track, and its diameter comes from the
    /// resolved [`SliderStyle`], not from any layout.
    ///
    /// The geometry is derived exactly as the body paints it, mirrored under
    /// RTL, so the report and the picture cannot drift.
    fn target_regions(&self, bounds: Rect) -> Vec<teksilo_core::partition::TargetRegion> {
        use teksilo_core::partition::TargetRegion;

        let mut regions = vec![TargetRegion::target(bounds, SLIDER_PART_BODY)];
        let diameter = self.thumb_diameter.get();
        if diameter <= 0.0 || !diameter.is_finite() {
            return regions;
        }
        let radius = diameter * 0.5;
        let range = self.max - self.min;
        let t = if range.abs() < f32::EPSILON {
            0.0
        } else {
            ((self.value.get() - self.min) / range).clamp(0.0, 1.0)
        };
        let (cx, cy) = match self.orientation {
            Orientation::Horizontal => {
                let usable = (bounds.width - diameter).max(0.0);
                let t = if self.cached_rtl.get() { 1.0 - t } else { t };
                (
                    bounds.x + radius + usable * t,
                    bounds.y + bounds.height * 0.5,
                )
            }
            Orientation::Vertical => {
                let usable = (bounds.height - diameter).max(0.0);
                (
                    bounds.x + bounds.width * 0.5,
                    bounds.y + radius + usable * t,
                )
            }
        };
        regions.push(TargetRegion::grab(
            Rect::new(cx - radius, cy - radius, diameter, diameter),
            SLIDER_PART_THUMB,
        ));
        regions
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::Slider);
        if let Some(ref label) = self.label {
            builder.set_name(label.resolve_now());
        }
        builder.set_numeric_value(self.value.get() as f64);
        builder.set_min_numeric_value(self.min as f64);
        builder.set_max_numeric_value(self.max as f64);
        // Publish both keyboard distances so Orca / VoiceOver can announce
        // "step by N" for an arrow and the coarser figure for a page key.
        // Both come from the same helpers the handlers use, so the announced
        // number and the applied one cannot drift.
        builder.set_numeric_value_step(self.effective_step() as f64);
        builder.set_numeric_value_jump(self.effective_page_step() as f64);
        let orientation = match self.orientation {
            Orientation::Horizontal => teksilo_core::accesskit::Orientation::Horizontal,
            Orientation::Vertical => teksilo_core::accesskit::Orientation::Vertical,
        };
        builder.set_orientation(orientation);
        // Framework a11y walker sets `set_disabled` from arena state.
        builder.add_action(teksilo_core::accesskit::Action::Increment);
        builder.add_action(teksilo_core::accesskit::Action::Decrement);
        // macOS gates `setAccessibilityValue:` settability on the node
        // supporting this action, so the advertisement is what makes
        // VoiceOver's value entry reachable at all.
        builder.add_action(teksilo_core::accesskit::Action::SetValue);
        builder.add_action(teksilo_core::accesskit::Action::Focus);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_canvas::Point;
    use teksilo_core::event::{Key, Modifiers};
    use teksilo_core::widget_tree::WidgetTree;

    #[test]
    fn focus_ring_only_under_focus_visible() {
        // `:focus-visible`: the keyboard-only focus ring (now derived live
        // from the input-modality signal, not a hover-at-focus snapshot).
        // Programmatic focus leaves `focus_visible` false → no ring; a key
        // press reveals it.
        let theme = teksilo_core::presets::intui::light();
        let ring = theme.colors.focus_ring.to_array();
        let mut tree = WidgetTree::new().with_theme(theme);
        let s = tree.add(Slider::new(Signal::new(50.0_f32), 0.0, 100.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));

        tree.focus(s);
        assert!(
            !frame_has_ring(&tree.render(), ring),
            "no focus ring while focus-visible is false (pointer modality)",
        );

        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert!(
            frame_has_ring(&tree.render(), ring),
            "focus ring shows under keyboard modality",
        );
    }

    /// Whether the focus-ring *stroke* (ring color + non-zero stroke width) is
    /// present. A plain color match is ambiguous: in IntUI `focus_ring` shares
    /// the `accent` RGBA, and the slider paints accent *fills* (track + thumb)
    /// — the ring is the only *stroked* shape in that color.
    fn frame_has_ring(frame: &teksilo_canvas::RenderFrame, color: [f32; 4]) -> bool {
        frame
            .shapes
            .iter()
            .any(|s| s.color == color && s.stroke_width > 0.0)
            || frame.cosmetic_lines.iter().any(|l| l.color == color)
    }

    #[test]
    fn keyboard_adjusts_value() {
        let value = Signal::new(50.0_f32);
        let mut tree = WidgetTree::new();
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0).step(10.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));

        tree.focus(s);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert!((value.get() - 60.0).abs() < 0.01, "value={}", value.get());

        tree.press_key(Key::ArrowLeft, Modifiers::NONE);
        assert!((value.get() - 50.0).abs() < 0.01);
    }

    #[test]
    fn home_end_jump_to_bounds() {
        let value = Signal::new(50.0_f32);
        let mut tree = WidgetTree::new();
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));

        tree.focus(s);
        tree.press_key(Key::Home, Modifiers::NONE);
        assert!((value.get() - 0.0).abs() < 0.01);

        tree.press_key(Key::End, Modifiers::NONE);
        assert!((value.get() - 100.0).abs() < 0.01);
    }

    /// The published `(numeric_value_step, numeric_value_jump)` pair.
    ///
    /// Read off the raw AccessKit node rather than `AccessibilityInfo`, which
    /// carries only role / name / actions. `accesskit::Node` is not `Clone`, so
    /// the values come back rather than the node.
    fn a11y_steps(
        tree: &mut WidgetTree,
        id: teksilo_core::widget_id::WidgetId,
    ) -> (Option<f64>, Option<f64>) {
        let update = tree.sync_accessibility();
        let nid = teksilo_core::accessibility::widget_id_to_node_id(id);
        update
            .nodes
            .iter()
            .find(|(node_id, _)| *node_id == nid)
            .map(|(_, n)| (n.numeric_value_step(), n.numeric_value_jump()))
            .expect("the slider publishes an AT node")
    }

    fn access(
        tree: &mut WidgetTree,
        id: teksilo_core::widget_id::WidgetId,
        action: teksilo_core::accesskit::Action,
        data: Option<teksilo_core::accesskit::ActionData>,
    ) -> bool {
        let mut ops = teksilo_core::window::NoopWindowOps;
        tree.dispatch_access_action(
            teksilo_core::accessibility::widget_id_to_node_id(id),
            action,
            data,
            &mut ops,
        )
    }

    #[test]
    fn page_keys_move_ten_arrow_steps() {
        // The default step is 1 % of the range and the default page is ten of
        // them, so a page is 10 % — `QAbstractSlider::pageStep`, `GtkScale`'s
        // page increment and `<input type=range>` in WebKit and Blink all land
        // on the same figure.
        let value = Signal::new(50.0_f32);
        let mut tree = WidgetTree::new();
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));
        tree.focus(s);

        tree.press_key(Key::PageUp, Modifiers::NONE);
        assert!((value.get() - 60.0).abs() < 0.01, "value={}", value.get());
        tree.press_key(Key::PageDown, Modifiers::NONE);
        assert!((value.get() - 50.0).abs() < 0.01, "value={}", value.get());
    }

    #[test]
    fn page_step_overrides_the_default() {
        let value = Signal::new(50.0_f32);
        let mut tree = WidgetTree::new();
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0).page_step(25.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));
        tree.focus(s);

        tree.press_key(Key::PageUp, Modifiers::NONE);
        assert!((value.get() - 75.0).abs() < 0.01, "value={}", value.get());
    }

    #[test]
    fn page_step_follows_an_explicit_step() {
        // Ten times *the effective step*, not ten times 1 % of the range.
        let value = Signal::new(50.0_f32);
        let mut tree = WidgetTree::new();
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0).step(2.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));
        tree.focus(s);

        tree.press_key(Key::PageUp, Modifiers::NONE);
        assert!((value.get() - 70.0).abs() < 0.01, "value={}", value.get());
    }

    #[test]
    fn paging_clamps_at_the_bounds() {
        let value = Signal::new(95.0_f32);
        let mut tree = WidgetTree::new();
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));
        tree.focus(s);

        tree.press_key(Key::PageUp, Modifiers::NONE);
        assert!((value.get() - 100.0).abs() < 0.01, "value={}", value.get());
    }

    #[test]
    fn an_accelerator_chord_leaves_the_value_alone() {
        // Behaviour change: `Ctrl+Home` used to drive the slider to its minimum
        // and report the key handled, so the chord never reached the
        // application's own Shortcut/Action pipeline.
        let value = Signal::new(50.0_f32);
        let mut tree = WidgetTree::new();
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0).step(10.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));
        tree.focus(s);

        for (key, mods) in [
            (Key::Home, Modifiers::CTRL),
            (Key::End, Modifiers::ALT),
            (Key::ArrowRight, Modifiers::CTRL),
            (Key::PageDown, Modifiers::SUPER),
        ] {
            tree.press_key(key, mods);
            assert!(
                (value.get() - 50.0).abs() < 0.01,
                "{key:?} with {mods:?} moved the value to {}",
                value.get()
            );
        }
    }

    #[test]
    fn a_shifted_arrow_still_steps() {
        // `Shift` is not a distinct chord on a slider, so it must not disable
        // the one the user pressed.
        let value = Signal::new(50.0_f32);
        let mut tree = WidgetTree::new();
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0).step(10.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));
        tree.focus(s);

        tree.press_key(Key::ArrowRight, Modifiers::SHIFT);
        assert!((value.get() - 60.0).abs() < 0.01, "value={}", value.get());
    }

    #[test]
    fn accessibility_publishes_the_page_step_as_the_value_jump() {
        // Orca and VoiceOver announce the coarse distance from this property,
        // and it must be the one the page keys actually apply.
        let mut tree = WidgetTree::new();
        let s = tree.add(Slider::new(Signal::new(50.0_f32), 0.0, 100.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));
        assert_eq!(a11y_steps(&mut tree, s), (Some(1.0), Some(10.0)));

        let mut tree = WidgetTree::new();
        let s = tree.add(Slider::new(Signal::new(50.0_f32), 0.0, 100.0).page_step(25.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));
        assert_eq!(a11y_steps(&mut tree, s).1, Some(25.0));
    }

    #[test]
    fn access_set_value_writes_and_snaps() {
        // AT-SPI publishes the `Value` interface off `numeric_value` alone, so
        // `Value.SetCurrentValue` — Orca's slider value entry — reached this
        // widget long before the action was advertised, and did nothing.
        use teksilo_core::accesskit::{Action, ActionData};
        let value = Signal::new(50.0_f32);
        let mut tree = WidgetTree::new();
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0).step(10.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));

        assert!(
            tree.accessibility_node(s)
                .actions()
                .contains(&Action::SetValue),
            "macOS gates AXValue settability on the advertisement"
        );

        assert!(access(
            &mut tree,
            s,
            Action::SetValue,
            Some(ActionData::NumericValue(73.0))
        ));
        assert!(
            (value.get() - 70.0).abs() < 0.01,
            "an AT write snaps to `step`, like a drag: {}",
            value.get()
        );

        // The string shape is what the automation `set_value` tool sends.
        assert!(access(
            &mut tree,
            s,
            Action::SetValue,
            Some(ActionData::Value("1000".into()))
        ));
        assert!((value.get() - 100.0).abs() < 0.01, "out of range clamps");

        // A value that is not a number is declined rather than silently
        // becoming one.
        assert!(!access(
            &mut tree,
            s,
            Action::SetValue,
            Some(ActionData::Value("seventy".into()))
        ));
        assert!((value.get() - 100.0).abs() < 0.01);
    }

    #[test]
    fn access_increment_survives_the_payload_handler() {
        // Increment and Decrement moved into the payload handler with
        // SetValue, because that is where `ActionData` rides. They used to have
        // to: the dispatcher called `on_access_action_request` *instead of*
        // `on_access_action`. It now fires both, but the pair still lives here
        // — one handler, one match, one place to read.
        use teksilo_core::accesskit::Action;
        let value = Signal::new(50.0_f32);
        let mut tree = WidgetTree::new();
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0).step(10.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));

        assert!(access(&mut tree, s, Action::Increment, None));
        assert!((value.get() - 60.0).abs() < 0.01, "value={}", value.get());
        assert!(access(&mut tree, s, Action::Decrement, None));
        assert!((value.get() - 50.0).abs() < 0.01, "value={}", value.get());
    }

    #[test]
    fn an_app_installed_access_action_still_fires_on_a_payload_widget() {
        // `.on_access_action(..)` is the app's hook; `on_access_action_request`
        // is what a widget reaches for when it needs `target_node` or `data`.
        // The dispatcher used to prefer the payload slot when it was set, so
        // installing an app handler on a `Slider`, `SpinBox`, `TextInput`,
        // `CodeEditor` or `TabBar` produced a handler that never ran and no
        // diagnostic anywhere.
        use std::cell::Cell;
        use std::rc::Rc;
        use teksilo_core::accesskit::Action;
        use teksilo_core::widget_builder::WidgetBuilder;

        let seen: Rc<Cell<u32>> = Rc::new(Cell::new(0));
        let seen_h = seen.clone();
        let value = Signal::new(50.0_f32);
        let mut tree = WidgetTree::new();
        let s = tree.add(
            Slider::new(value.clone(), 0.0, 100.0)
                .step(10.0)
                .on_access_action(move |action, _ctx| {
                    if action == Action::Increment {
                        seen_h.set(seen_h.get() + 1);
                    }
                    EventResponse::Ignored
                }),
        );
        tree.layout(SizeProposal::exact(200.0, 60.0));

        assert!(access(&mut tree, s, Action::Increment, None));
        assert_eq!(seen.get(), 1, "the app's handler runs");
        assert!(
            (value.get() - 60.0).abs() < 0.01,
            "and the widget's own still steps: {}",
            value.get()
        );
    }

    #[test]
    fn rtl_mirrors_the_arrows_the_pointer_and_the_fill_together() {
        // A horizontal slider's minimum sits at the *leading* edge, which is
        // the right one in a right-to-left window. All three readings of that
        // — the arrows, the click-to-jump and the painted fill — mirror off
        // the same answer; mirroring any one alone would leave the `Left` key
        // and a leftward drag moving the thumb in opposite directions.
        let value = Signal::new(50.0_f32);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.set_layout_direction(teksilo_core::environment::LayoutDirection::RightToLeft);
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0).step(10.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));
        tree.render();
        tree.focus(s);

        tree.press_key(Key::ArrowLeft, Modifiers::NONE);
        assert!(
            (value.get() - 60.0).abs() < 0.01,
            "under RTL the leftward arrow increases, got {}",
            value.get()
        );
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert!((value.get() - 50.0).abs() < 0.01);

        // The pointer reads from the other end too: a click near the *right*
        // edge is near the minimum under RTL, where it would be the maximum
        // left-to-right.
        let p = Point::new(190.0, 30.0);
        tree.pointer_move(p);
        tree.dispatch_event(WidgetEvent::pointer_down(
            p,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        tree.dispatch_event(WidgetEvent::pointer_up(
            p,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        assert!(
            value.get() < 20.0,
            "a click near the right edge is near the minimum under RTL, got {}",
            value.get()
        );
    }

    #[test]
    fn an_at_write_lands_on_the_grid_the_arrows_walk() {
        // The advertised `numeric_value_step`, the arrows and `Increment` all
        // move by `effective_step` — 1 % of the range when no step is
        // configured. The write snapped to the *configured* step instead, so on
        // a stepless slider it landed between two values every other path could
        // reach, and Orca then announced a number its own Up arrow could never
        // produce.
        use teksilo_core::accesskit::{Action, ActionData};
        let value = Signal::new(50.0_f32);
        let mut tree = WidgetTree::new();
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));

        assert!(access(
            &mut tree,
            s,
            Action::SetValue,
            Some(ActionData::NumericValue(37.3))
        ));
        assert!(
            (value.get() - 37.0).abs() < 0.01,
            "a stepless 0..100 slider steps by 1, so the write snaps there: {}",
            value.get()
        );

        // …and the arrow lands on the same grid from there.
        tree.focus(s);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert!((value.get() - 38.0).abs() < 0.01, "value={}", value.get());
    }

    #[test]
    fn a_vertical_slider_puts_its_maximum_at_the_top() {
        // Qt's `QSlider`, GTK4's `GtkScale`, the Win32 trackbar and
        // `<input type=range>` all put a vertical slider's minimum at the
        // bottom, and it is what makes `ArrowUp` — which `range_nav` reports as
        // an increase — raise the thumb. Growing the thumb position with the
        // value sent it *down*, so the keys and the pointer both drove the
        // control backwards.
        let value = Signal::new(50.0_f32);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0).orientation(Orientation::Vertical));
        tree.layout(SizeProposal::exact(60.0, 200.0));
        tree.render();

        // A click near the top is near the maximum.
        let top = Point::new(30.0, 12.0);
        tree.pointer_move(top);
        for ev in [
            WidgetEvent::pointer_down(top, PointerButton::Primary, Modifiers::NONE),
            WidgetEvent::pointer_up(top, PointerButton::Primary, Modifiers::NONE),
        ] {
            tree.dispatch_event(ev);
        }
        assert!(
            value.get() > 90.0,
            "a click near the top is near the maximum, got {}",
            value.get()
        );

        // …and the arrow that increases the value moves towards it.
        value.set(50.0);
        tree.focus(s);
        tree.press_key(Key::ArrowUp, Modifiers::NONE);
        let raised = value.get();
        assert!(raised > 50.0, "ArrowUp increases, got {raised}");

        // The painted thumb agrees: higher value, smaller y.
        let thumb_y = |tree: &mut WidgetTree| -> f32 {
            let radius = crate::styles::recipe_slider_style::SLIDER_THUMB_DIAMETER;
            tree.render()
                .shapes
                .iter()
                .filter(|q| (q.screen[3] - radius).abs() < 1.0)
                .map(|q| q.screen[1])
                .fold(f32::MAX, f32::min)
        };
        value.set(10.0);
        let low = thumb_y(&mut tree);
        value.set(90.0);
        let high = thumb_y(&mut tree);
        assert!(
            high < low,
            "the thumb rises as the value grows: y({}) at 90 vs y({}) at 10",
            high,
            low
        );
    }

    #[test]
    fn a_vertical_slider_ignores_the_layout_direction() {
        // `Up`/`Down` name points in value space; only the horizontal pair
        // mirrors.
        let value = Signal::new(50.0_f32);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.set_layout_direction(teksilo_core::environment::LayoutDirection::RightToLeft);
        let s = tree.add(
            Slider::new(value.clone(), 0.0, 100.0)
                .step(10.0)
                .orientation(Orientation::Vertical),
        );
        tree.layout(SizeProposal::exact(60.0, 200.0));
        tree.focus(s);

        tree.press_key(Key::ArrowUp, Modifiers::NONE);
        assert!((value.get() - 60.0).abs() < 0.01, "value={}", value.get());
    }

    #[test]
    fn track_click_sets_value() {
        let value = Signal::new(0.0_f32);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));
        // Render to trigger paint() which caches bounds for event handling
        tree.render();

        // Click at the widget center
        tree.click(s);

        // Value should be approximately 50 (midpoint of 0..100)
        let val = value.get();
        assert!(
            (val - 50.0).abs() < 15.0,
            "track click at center should set value near 50, got {}",
            val
        );
    }

    #[test]
    fn track_click_sets_value_at_nonzero_origin() {
        // Regression for the widget-local coordinate migration: a slider
        // offset from the window origin must still map a click correctly.
        use crate::primitives::{FixedSize, HStack};
        use teksilo_canvas::Point;
        use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};

        let value = Signal::new(0.0_f32);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let sid = tree.add(Slider::new(value.clone(), 0.0, 100.0));
        let _row = tree.add(
            HStack::new()
                .child(FixedSize::new().width(40.0).height(60.0))
                .add_child(sid),
        );
        tree.layout(SizeProposal::exact(240.0, 60.0));
        tree.render();

        // The slider sits at window x ∈ [40, 240] (width 200). Its local
        // centre (x = 100) is window x = 140 → value 50, independent of
        // the thumb radius.
        let b = tree.bounds(sid);
        assert!(
            (b.x - 40.0).abs() < 0.5,
            "slider should be offset, x={}",
            b.x
        );
        for ev in [
            WidgetEvent::pointer_down(
                Point::new(140.0, 30.0),
                PointerButton::Primary,
                Modifiers::NONE,
            ),
            WidgetEvent::pointer_up(
                Point::new(140.0, 30.0),
                PointerButton::Primary,
                Modifiers::NONE,
            ),
        ] {
            tree.dispatch_event(ev);
        }
        assert!(
            (value.get() - 50.0).abs() < 1.0,
            "click at the offset slider's centre should set ~50, got {}",
            value.get()
        );
    }

    #[test]
    fn accessibility() {
        let value = Signal::new(25.0_f32);
        let mut tree = WidgetTree::new();
        let s = tree.add(Slider::new(value, 0.0, 100.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));
        let info = tree.accessibility_node(s);
        assert_eq!(info.role(), teksilo_core::accesskit::Role::Slider);
    }

    #[test]
    fn step_snaps_value() {
        let value = Signal::new(0.0_f32);
        let mut tree = WidgetTree::new();
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0).step(25.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));

        tree.focus(s);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert!((value.get() - 25.0).abs() < 0.01);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert!((value.get() - 50.0).abs() < 0.01);
    }

    #[test]
    fn thumb_drag_updates_value() {
        let theme = teksilo_core::presets::intui::light();
        let thumb_radius = crate::styles::recipe_slider_style::SLIDER_THUMB_DIAMETER * 0.5;
        let value = Signal::new(50.0_f32);
        let mut tree = WidgetTree::new().with_theme(theme);
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));
        tree.render(); // cache bounds for event handling

        let bounds = tree.bounds(s);
        // Thumb center for value=50: bounds.x + r + (width - 2r) * 0.5
        let thumb_cx = bounds.x + thumb_radius + (bounds.width - thumb_radius * 2.0) * 0.5;
        let center_y = bounds.y + bounds.height / 2.0;

        // Pointer down on thumb
        tree.pointer_down_button(Point::new(thumb_cx, center_y), PointerButton::Primary);

        // Drag to 75% position. DragRecognizer needs one move past its
        // 5 px threshold to emit `DragStarted` (which carries the *down*
        // position, leaving value at 50%), and a second move to emit
        // `DragMoved` — the latter is what actually drives the value.
        let target_x = bounds.x + thumb_radius + (bounds.width - thumb_radius * 2.0) * 0.75;
        tree.pointer_move(Point::new(thumb_cx + 10.0, center_y));
        tree.pointer_move(Point::new(target_x, center_y));

        let val = value.get();
        assert!(
            (val - 75.0).abs() < 5.0,
            "dragging to 75% should set value near 75, got {}",
            val
        );

        // Release
        tree.pointer_up_button(Point::new(target_x, center_y), PointerButton::Primary);
    }

    #[test]
    fn accessibility_has_actions() {
        let value = Signal::new(25.0_f32);
        let mut tree = WidgetTree::new();
        let s = tree.add(Slider::new(value, 0.0, 100.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));
        let info = tree.accessibility_node(s);
        assert!(
            info.actions()
                .contains(&teksilo_core::accesskit::Action::Increment)
        );
        assert!(
            info.actions()
                .contains(&teksilo_core::accesskit::Action::Decrement)
        );
    }

    // -----------------------------------------------------------------
    // Touch: the manipulator contract
    // -----------------------------------------------------------------

    /// A finger drag that starts on a slider inside a scroller adjusts the
    /// slider, from the **first move sample**, and scrolls nothing.
    ///
    /// The end-to-end behaviour the touch programme promises for a continuous
    /// manipulator. What delivers it is the press capture the slider's drag
    /// takes: it makes the slider the innermost member of the pointer's
    /// sequence, which both stops the enclosing claimant winning the gesture
    /// and means no long-press timer is ever interposed. The drag is deliberately
    /// **vertical-dominant** — straight down the axis the scroller claims and
    /// far past the 36 dp pan slop — because a drag along the slider's own axis
    /// is not contested by a vertical claimant and would prove nothing.
    #[test]
    fn a_finger_drag_on_a_slider_inside_a_scroller_adjusts_the_slider() {
        use crate::button::press_test_support::{finger, touch};
        use teksilo_core::event::EventResponse;
        use teksilo_core::pointer::PointerPhase;
        use teksilo_core::pointer::touch_action::PanClaim;
        use teksilo_core::widget_builder::WidgetBuilder;

        let scrolled = Rc::new(Cell::new(0_u32));
        let count = scrolled.clone();
        let value = Signal::new(0.0_f32);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let slider = tree.add(Slider::new(value.clone(), 0.0, 100.0));
        let _list = tree.add(
            crate::primitives::VStack::new()
                .add_child(slider)
                .scroll_container(teksilo_core::pointer::touch_action::PanAxes::BOTH)
                .pan_claim(PanClaim::vertical())
                .on_scroll(move |_e, _c| {
                    count.set(count.get() + 1);
                    EventResponse::Handled
                }),
        );
        tree.layout(SizeProposal::exact(200.0, 400.0));
        let b = tree.bounds(slider);
        let start = Point::new(b.x + 20.0, b.center().y);
        let id = finger();
        tree.dispatch_pointer(touch(id, PointerPhase::Down, start, 0));

        // One move, already past the coarse 18 dp drag slop. The value must
        // have moved on *this* sample: a drag deferred to the 500 ms long-press
        // timer would leave it at zero here.
        tree.dispatch_pointer(touch(
            id,
            PointerPhase::Move,
            Point::new(start.x + 30.0, start.y + 45.0),
            20,
        ));
        assert!(
            value.get() > 0.0,
            "the drag must arm on the first move, not after a long press (value {})",
            value.get(),
        );

        // …and keep going, vertically far past the 36 dp pan slop.
        for (i, (dx, dy)) in [(60.0_f32, 90.0_f32), (90.0, 140.0)]
            .into_iter()
            .enumerate()
        {
            let at = Point::new(start.x + dx, start.y + dy);
            tree.dispatch_pointer(touch(id, PointerPhase::Move, at, 40 + i as u64 * 20));
        }
        tree.dispatch_pointer(touch(
            id,
            PointerPhase::Up,
            Point::new(start.x + 90.0, start.y + 140.0),
            100,
        ));
        assert!(
            value.get() > 40.0,
            "the finger drag did not reach the slider (value {})",
            value.get(),
        );
        assert_eq!(scrolled.get(), 0, "and it must not have scrolled the list");
    }

    /// **`touch_action(NONE)`'s own test.** Two contacts landing on a
    /// manipulator start no pinch on the surface under it.
    ///
    /// This is the one consumer of the declaration that the slider's press
    /// capture does not already cover: `feed_pinch` asks
    /// `effective_touch_action(hit target).allows_pinch()` on the contact
    /// itself, before any capture or arbitration exists. Relax the slider to
    /// `AUTO` and the pinch starts.
    #[test]
    fn two_contacts_on_a_slider_pinch_nothing_underneath() {
        use crate::button::press_test_support::{finger, touch};
        use teksilo_core::gesture::PinchPhase;
        use teksilo_core::pointer::PointerPhase;
        use teksilo_core::widget_builder::WidgetBuilder;

        let started = Rc::new(Cell::new(0_u32));
        let n = started.clone();
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let slider = tree.add(Slider::new(Signal::new(0.0_f32), 0.0, 100.0));
        let _surface = tree.add(crate::primitives::ZStack::new().add_child(slider).on_pinch(
            move |phase, _c| {
                if matches!(phase, PinchPhase::Started { .. }) {
                    n.set(n.get() + 1);
                }
            },
        ));
        tree.layout(SizeProposal::exact(200.0, 200.0));
        let b = tree.bounds(slider);
        let a = finger();
        let c = finger();
        tree.dispatch_pointer(touch(
            a,
            PointerPhase::Down,
            Point::new(b.x + 40.0, b.center().y),
            0,
        ));
        tree.dispatch_pointer(touch(
            c,
            PointerPhase::Down,
            Point::new(b.x + 150.0, b.center().y),
            5,
        ));

        assert_eq!(
            started.get(),
            0,
            "a pinch started on a manipulator must not reach the surface under it",
        );
        assert!(
            !tree.touch_pinch_active(),
            "…and no pinch may be running at all",
        );
    }

    /// The mirroring reaches the mounted widget, off the same layout direction
    /// the pointer path reads at event time — which is what makes the keys and
    /// the drag agree by construction rather than by coincidence. The chord
    /// table itself is `common::range_nav`, shared with every other bounded
    /// scalar and tested there.
    #[test]
    fn an_rtl_slider_raises_its_value_on_the_leftward_arrow() {
        use teksilo_core::environment::LayoutDirection;

        for (direction, raising, lowering) in [
            (
                LayoutDirection::LeftToRight,
                Key::ArrowRight,
                Key::ArrowLeft,
            ),
            (
                LayoutDirection::RightToLeft,
                Key::ArrowLeft,
                Key::ArrowRight,
            ),
        ] {
            let value = Signal::new(50.0_f32);
            let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
            tree.set_layout_direction(direction);
            let s = tree.add(Slider::new(value.clone(), 0.0, 100.0).step(10.0));
            tree.layout(SizeProposal::exact(200.0, 40.0));
            tree.focus(s);
            tree.press_key(raising, Modifiers::NONE);
            assert_eq!(value.get(), 60.0, "{direction:?}: {raising:?} must raise");
            tree.press_key(lowering, Modifiers::NONE);
            tree.press_key(lowering, Modifiers::NONE);
            assert_eq!(value.get(), 40.0, "{direction:?}: {lowering:?} must lower");
        }
    }

    /// A horizontal slider's minimum sits at the **leading** edge, which is the
    /// right-hand one under RTL. The painted knob and the value the press maps
    /// to must agree; before this they ran opposite ways.
    #[test]
    fn the_value_axis_mirrors_under_rtl() {
        use teksilo_core::environment::LayoutDirection;

        let value = Signal::new(0.0_f32);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.set_layout_direction(LayoutDirection::RightToLeft);
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0));
        tree.layout(SizeProposal::exact(200.0, 40.0));
        let b = tree.bounds(s);

        // A tap near the RIGHT edge is the minimum under RTL.
        tree.pointer_down_button(
            Point::new(b.right() - 4.0, b.center().y),
            PointerButton::Primary,
        );
        tree.pointer_up_button(
            Point::new(b.right() - 4.0, b.center().y),
            PointerButton::Primary,
        );
        assert!(
            value.get() < 10.0,
            "RTL minimum is the right edge, got {}",
            value.get()
        );

        // …and the LEFT edge is the maximum.
        tree.pointer_down_button(Point::new(b.x + 4.0, b.center().y), PointerButton::Primary);
        tree.pointer_up_button(Point::new(b.x + 4.0, b.center().y), PointerButton::Primary);
        assert!(
            value.get() > 90.0,
            "RTL maximum is the left edge, got {}",
            value.get()
        );
    }

    /// What the body actually painted, read out of the render frame.
    ///
    /// The default `SliderStyle` draws track, fill and knob as three rounded
    /// rects on one canvas, so nothing in the widget tree can be measured to
    /// find them; the frame is the only place the picture exists. They are told
    /// apart by the two things the recipe fixes: the track carries
    /// `surface_sunken`, and of the two accent-coloured rects the knob is the
    /// square one (`thumb_diameter` on both axes) while the fill is
    /// `track_height` tall.
    struct PaintedSlider {
        track: teksilo_canvas::Rect,
        fill: Option<teksilo_canvas::Rect>,
        thumb: teksilo_canvas::Rect,
    }

    fn painted_slider(
        direction: teksilo_core::environment::LayoutDirection,
        value: f32,
    ) -> PaintedSlider {
        use crate::styles::recipe_slider_style::{SLIDER_THUMB_DIAMETER, SLIDER_TRACK_HEIGHT};

        let theme = teksilo_core::presets::intui::light();
        let accent = theme.colors.accent.to_array();
        let sunken = theme.colors.surface_sunken.to_array();
        let mut tree = WidgetTree::new().with_theme(theme);
        tree.set_layout_direction(direction);
        let _s = tree.add(Slider::new(Signal::new(value), 0.0, 100.0));
        tree.layout(SizeProposal::exact(200.0, 40.0));
        let frame = tree.render();
        let rect = |q: &teksilo_canvas::ShapeQuad| {
            teksilo_canvas::Rect::new(q.screen[0], q.screen[1], q.screen[2], q.screen[3])
        };
        let track = frame
            .shapes
            .iter()
            .find(|q| q.color == sunken)
            .map(rect)
            .expect("the body paints a track");
        let thumb = frame
            .shapes
            .iter()
            .find(|q| {
                q.color == accent
                    && (q.screen[2] - SLIDER_THUMB_DIAMETER).abs() < 0.01
                    && (q.screen[3] - SLIDER_THUMB_DIAMETER).abs() < 0.01
            })
            .map(rect)
            .expect("the body paints a knob");
        let fill = frame
            .shapes
            .iter()
            .find(|q| {
                q.color == accent
                    && (q.screen[3] - SLIDER_TRACK_HEIGHT).abs() < 0.01
                    && q.screen[2] > 0.0
            })
            .map(rect);
        PaintedSlider { track, fill, thumb }
    }

    /// The reported knob is where the body painted it, in both directions.
    ///
    /// Two halves, and the second is what makes the first mean anything: the
    /// report is derived in `Slider::target_regions` from a cached rtl flag and
    /// the picture is derived in `SliderBody::paint` from the paint context's
    /// own, so "the report and the picture cannot drift" is a claim about two
    /// separate pieces of arithmetic. Reading only the report would leave the
    /// paint free to run the other way.
    #[test]
    fn the_reported_thumb_mirrors_with_the_paint() {
        use teksilo_core::environment::LayoutDirection;

        fn thumb_centre(direction: LayoutDirection) -> f32 {
            let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
            tree.set_layout_direction(direction);
            let s = tree.add(Slider::new(Signal::new(0.0_f32), 0.0, 100.0));
            tree.layout(SizeProposal::exact(200.0, 40.0));
            let bounds = tree.bounds(s);
            let regions = tree.widget_target_regions(s);
            let thumb = regions
                .iter()
                .find(|r| r.part == SLIDER_PART_THUMB)
                .expect("the slider reports its knob");
            thumb.rect.center().x - bounds.x
        }

        let ltr = thumb_centre(LayoutDirection::LeftToRight);
        let rtl = thumb_centre(LayoutDirection::RightToLeft);
        assert!(
            ltr < 20.0,
            "at the minimum, LTR puts the knob at the left ({ltr})"
        );
        assert!(rtl > 180.0, "and RTL puts it at the right ({rtl})");

        // …and the paint agrees, at a value where being off by the mirror is
        // three quarters of the track rather than a rounding error.
        for direction in [LayoutDirection::LeftToRight, LayoutDirection::RightToLeft] {
            let painted = painted_slider(direction, 25.0);
            let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
            tree.set_layout_direction(direction);
            let s = tree.add(Slider::new(Signal::new(25.0_f32), 0.0, 100.0));
            tree.layout(SizeProposal::exact(200.0, 40.0));
            let reported = tree
                .widget_target_regions(s)
                .into_iter()
                .find(|r| r.part == SLIDER_PART_THUMB)
                .expect("the slider reports its knob");
            assert!(
                (reported.rect.center().x - painted.thumb.center().x).abs() < 0.51,
                "{direction:?}: the slider reports its knob at {} and paints it at {}",
                reported.rect.center().x,
                painted.thumb.center().x,
            );
        }
    }

    /// Under RTL a horizontal slider's minimum is the **right** edge, so the
    /// filled part of the track is the stretch between the knob and that edge.
    ///
    /// The knob and the fill are mirrored by two separate expressions in
    /// `SliderBody::paint`, and reverting either one on its own leaves a
    /// picture that is merely wrong rather than crashing: a knob a quarter of
    /// the way along an RTL track, or a fill running from the wrong edge and
    /// three times too long. Both are pinned here, against a value far from the
    /// midpoint so neither can hide behind symmetry.
    #[test]
    fn the_rtl_fill_runs_from_the_knob_to_the_leading_edge() {
        use teksilo_core::environment::LayoutDirection;

        let ltr = painted_slider(LayoutDirection::LeftToRight, 25.0);
        let ltr_fill = ltr.fill.expect("a quarter-full slider paints a fill");
        assert!(
            (ltr_fill.x - ltr.track.x).abs() < 0.51,
            "LTR fills from the left edge of the track, not from {}",
            ltr_fill.x,
        );
        assert!(
            (ltr_fill.width - ltr.track.width * 0.25).abs() < 0.51,
            "LTR fills a quarter of the track, not {} of {}",
            ltr_fill.width,
            ltr.track.width,
        );
        assert!(
            (ltr_fill.right() - ltr.thumb.center().x).abs() < 0.51,
            "LTR: the fill ends at the knob ({} vs {})",
            ltr_fill.right(),
            ltr.thumb.center().x,
        );

        let rtl = painted_slider(LayoutDirection::RightToLeft, 25.0);
        let rtl_fill = rtl.fill.expect("a quarter-full slider paints a fill");
        assert!(
            (rtl.thumb.center().x - (rtl.track.x + rtl.track.width * 0.75)).abs() < 0.51,
            "RTL puts a quarter-value knob three quarters along the track, not at {}",
            rtl.thumb.center().x,
        );
        assert!(
            (rtl_fill.right() - rtl.track.right()).abs() < 0.51,
            "RTL fills to the right edge of the track, not to {}",
            rtl_fill.right(),
        );
        assert!(
            (rtl_fill.width - rtl.track.width * 0.25).abs() < 0.51,
            "RTL fills a quarter of the track, not {} of {}",
            rtl_fill.width,
            rtl.track.width,
        );
        assert!(
            (rtl_fill.x - rtl.thumb.center().x).abs() < 0.51,
            "RTL: the fill starts at the knob ({} vs {})",
            rtl_fill.x,
            rtl.thumb.center().x,
        );
    }

    /// The reported knob is the size the **style** says it painted.
    ///
    /// `Slider` cannot see the knob: it is three rounded rects on one canvas,
    /// and the only place its diameter exists is the resolved `SliderStyle`.
    /// So the widget asks — `thumb_diameter_for`, at build time, while the
    /// theme is still in reach — and the source comment says why it asks
    /// rather than baking in the recipe's design constant: "so a custom
    /// `SliderStyle` with a different thumb size keeps drag hit-testing
    /// aligned".
    ///
    /// The expectation therefore has to come from the style too. A test that
    /// re-derives it from the widget's own cached field asserts only that the
    /// field equals itself, and a knob frozen at any plausible-looking constant
    /// — the 24 dp conformance floor, say, against the shipped 14 dp — passes
    /// it while every custom style's drag lands on the wrong pixel.
    #[test]
    fn the_reported_knob_is_the_size_the_style_reports() {
        use teksilo_core::styles::{
            SliderOrientation, SliderStyle, SliderStyleConfig, SliderVariant,
        };

        fn config() -> SliderStyleConfig {
            SliderStyleConfig {
                value_normalized: Signal::new(0.5),
                is_hovered: Signal::new(false),
                is_dragging: Signal::new(false),
                is_disabled: Signal::new(false),
                focus_origin: Signal::new(None),
                orientation: SliderOrientation::Horizontal,
                tick_count: None,
                variant: SliderVariant::Continuous,
            }
        }

        /// The knob region a slider reports, built with the default style or
        /// with one that declares `diameter`.
        fn reported_knob(diameter: Option<f32>) -> Rect {
            let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
            let mut slider = Slider::new(Signal::new(50.0_f32), 0.0, 100.0);
            if let Some(diameter) = diameter {
                slider = slider.style(FatKnob(diameter));
            }
            let s = tree.add(slider);
            tree.layout(SizeProposal::exact(200.0, 60.0));
            tree.widget_target_regions(s)
                .into_iter()
                .find(|r| r.part == SLIDER_PART_THUMB)
                .expect("the slider reports its knob")
                .rect
        }

        /// A style that paints the stock body but declares a knob of its own.
        /// The whole point of the `thumb_diameter` hook.
        struct FatKnob(f32);

        impl SliderStyle for FatKnob {
            fn make_body(
                &self,
                cfg: &SliderStyleConfig,
                ctx: &mut teksilo_core::build_context::BuildContext,
            ) -> WidgetId {
                crate::styles::RecipeSliderStyle::default().make_body(cfg, ctx)
            }

            fn thumb_diameter(&self, _cfg: &SliderStyleConfig) -> f32 {
                self.0
            }
        }

        // The shipped default: the number in the report is the number the
        // default style answers with, not a constant that happens to look
        // right today.
        let shipped = crate::styles::RecipeSliderStyle::default().thumb_diameter(&config());
        let knob = reported_knob(None);
        assert_eq!(
            (knob.width, knob.height),
            (shipped, shipped),
            "the default style reports a {shipped} dp knob and the region is {}x{}",
            knob.width,
            knob.height,
        );

        // And a style that disagrees is obeyed. 30 dp is nothing the widget
        // could arrive at on its own: not the recipe's 14, not the 24 dp
        // conformance floor, not the 6 dp grab floor the audit checks against.
        let fat = reported_knob(Some(30.0));
        assert_eq!(
            (fat.width, fat.height),
            (30.0, 30.0),
            "a custom style's 30 dp knob reported as {}x{}",
            fat.width,
            fat.height,
        );
        assert_ne!(
            fat.width, shipped,
            "the probe only discriminates while the two styles disagree",
        );
    }

    /// The slider reports its **whole node** as the press surface, beside the
    /// knob.
    ///
    /// It is the first of the two regions and it is claimed twice in prose —
    /// the module header's "the whole node as the press surface" and
    /// `target_regions`' "it is reported whole, and it is what the conformance
    /// audit measures". Both are load-bearing: a press anywhere on the body
    /// jumps the value, so the body is the target a coarse-press router aims
    /// at, and it is the rect the audit measures the 24 dp floor against. A
    /// slider that reported only its knob would advertise a 14 dp control.
    ///
    /// The tests either side of this one do not cover it: the floor sweep
    /// iterates whatever comes back, so it passes vacuously on an empty-but-
    /// for-the-knob report, and the other two search for the thumb part alone.
    /// The colour-picker strips have exactly this test
    /// (`the_hue_strip_reports_its_body_and_its_thumb`).
    #[test]
    fn the_reported_body_is_the_whole_node() {
        use teksilo_core::partition::TargetRegion;

        fn body_of(regions: &[TargetRegion]) -> &TargetRegion {
            regions
                .iter()
                .find(|r| r.part == SLIDER_PART_BODY)
                .expect("the slider reports its body as a press surface")
        }

        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let value = Signal::new(50.0_f32);
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0));
        tree.layout(SizeProposal::exact(200.0, 40.0));

        let bounds = tree.bounds(s);
        let regions = tree.widget_target_regions(s);
        assert_eq!(regions.len(), 2, "body and knob, got {regions:?}");

        let body = body_of(&regions);
        assert_eq!(
            body.rect, bounds,
            "the body region is the node's own rect, not a slice of it",
        );
        assert_eq!(
            body.role,
            teksilo_tokens::TargetRole::Target,
            "a press surface is a Target — a Grab would be audited against the \
             6 dp floor instead of the 24 dp one",
        );

        // The body is the node, so it does not follow the value the way the
        // knob does — a report that mixed the two up is caught here.
        let knob = regions
            .iter()
            .find(|r| r.part == SLIDER_PART_THUMB)
            .expect("the slider reports its knob")
            .rect;
        assert!(
            body.rect.width > knob.width,
            "the body ({:?}) should span the whole node the knob ({knob:?}) slides along",
            body.rect,
        );
        value.set(90.0);
        assert_eq!(
            body_of(&tree.widget_target_regions(s)).rect,
            bounds,
            "the body stays the whole node as the value moves",
        );
    }

    /// Every region the slider reports clears the floor its role is audited
    /// against, at the density CI runs at.
    #[test]
    fn the_reported_regions_clear_their_floors_at_compact() {
        let theme = teksilo_core::presets::intui::light();
        let tokens = theme.input;
        let mut tree = WidgetTree::new().with_theme(theme);
        let s = tree.add(Slider::new(Signal::new(50.0_f32), 0.0, 100.0));
        tree.layout(SizeProposal::exact(200.0, 40.0));
        for region in tree.widget_target_regions(s) {
            let floor = match region.role {
                teksilo_tokens::TargetRole::Target => tokens.min_target_conformance,
                teksilo_tokens::TargetRole::Grab => tokens.grab_size,
                teksilo_tokens::TargetRole::Decoration => continue,
            };
            let min = region.rect.width.min(region.rect.height);
            assert!(
                min >= floor,
                "part {} ({:?}) measured {min}, under its {floor} dp floor",
                region.part,
                region.role,
            );
        }
    }
}
