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
//! ## Value readout and reset
//!
//! [`value_tooltip`](Slider::value_tooltip) shows the value as text beside the
//! thumb — "-3.0 dB" on an equalizer band — while the pointer is over the
//! slider, while a drag is moving it, and while it has keyboard focus. It is up
//! at once rather than after a hover delay, follows the thumb wherever the
//! slider goes, and changes as the value does. It takes no input: a press or a
//! hover on it belongs to what is under it, and Escape takes it down without
//! keeping the key from the field the user pressed it for. The same text
//! becomes the slider's accessible value, beside the number, so a reader that
//! speaks a value's text says "-3.0 dB" rather than "-3".
//!
//! [`default_value`](Slider::default_value) adds a way back: a double-click on
//! the slider, or the "Reset to default" accessibility action, puts the value
//! back to the default, as a user change, so [`on_change`](Slider::on_change)
//! reports it.
//!
//! ```rust
//! # use teksilo_core::signal::Signal;
//! # use teksilo_i18n::lit;
//! # use teksilo_widgets::Slider;
//! let gain = Signal::new(0.0_f32);
//! let _band = Slider::new(gain, -12.0, 12.0)
//!     .step(0.5)
//!     .label(lit!("Low shelf"))
//!     .value_tooltip(|db| lit!(format!("{db:+.1} dB")))
//!     .default_value(0.0);
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
use teksilo_core::binding::BindingLevel;
use teksilo_core::event::{EventResponse, Key, PointerButton, WidgetEvent};
use teksilo_core::focus::FocusOrigin;
use teksilo_core::gesture::{DragPhase, TapStreak};
use teksilo_core::pointer::PointerId;
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

mod readout;

use readout::{Readout, ReadoutInputs, ValueFormat};

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

/// AccessKit custom-action id of "Reset to default", advertised when a
/// [`default_value`](Slider::default_value) is set.
///
/// Fixed, so an assistive client's recorded id keeps meaning the same thing,
/// and away from the low numbers: an application's own
/// `.access_custom_action(..)` entries are numbered from 0 by position, and the
/// dispatcher hands one `CustomAction(id)` to the widget and to those entries
/// alike.
const RESET_ACTION_ID: i32 = 1000;

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
    on_change: Option<Rc<dyn Fn(f32, &mut teksilo_core::widget::EventContext)>>,
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
    /// Formats the value for the live readout and the accessible value text.
    value_format: Option<ValueFormat>,
    /// The value the reset gesture and the reset action put back.
    default_value: Option<f32>,
    /// The arena's effective enabled state, captured at build so
    /// `accessibility()` can withdraw the reset action from a disabled slider.
    effective_enabled: Option<Signal<bool>>,
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
            on_change: None,
            dragging: Signal::new(false),
            focused: Signal::new(false),
            cached_bounds: Rc::new(Cell::new(Rect::ZERO)),
            cached_rtl: Rc::new(Cell::new(false)),
            thumb_diameter: Cell::new(0.0),
            body_id: None,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            value_format: None,
            default_value: None,
            effective_enabled: None,
        }
    }

    /// Run `f` for every value this control produces under the **user's**
    /// hand, with an `EventContext`, so it can do what a bare `Signal` write
    /// cannot (`ctx.send_intent(...)`, opening a window). Fires for a track
    /// click, for each step of a drag, for the arrows, for an assistive
    /// technology's `Increment` / `Decrement` / `SetValue`, and for a reset to
    /// the [`default_value`](Self::default_value).
    ///
    /// **A drag fires this repeatedly** — once per value it actually produces,
    /// not once per pointer sample, since a write that changes nothing reports
    /// nothing. It is still the wrong place for work that should happen once
    /// per interaction: persisting to disk, a network call, an undo entry.
    /// There is no commit-on-release callback yet; observe the signal and do
    /// that work when the value settles.
    ///
    /// Does **not** fire for programmatic writes to the bound signal — there is
    /// no event in flight to carry. Observe the signal for that.
    pub fn on_change(
        mut self,
        f: impl Fn(f32, &mut teksilo_core::widget::EventContext) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(f));
        self
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
    /// wins and clears the others. Independent of
    /// [`value_tooltip`](Self::value_tooltip), which neither replaces nor is
    /// replaced by any of them.
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

    /// Show the value as text by the thumb, formatted by `format`, while the
    /// slider is hovered, dragged, or focused from the keyboard.
    ///
    /// Unlike a [`tooltip`](Self::tooltip), the readout is up at once, with no
    /// hover delay; a press does not close it, so it stays up through a drag;
    /// and it moves with the thumb and changes as the value does, without a
    /// rebuild. It sits above a horizontal slider's thumb, or below it near
    /// the top of the window, and beside a vertical slider, on the
    /// inline-start side when there is room, so it covers neither the thumb
    /// nor the track. It is re-placed on every layout pass from where the
    /// slider is then, so it follows a value the application writes, a scroll
    /// that moves the slider and a resize that stretches it.
    ///
    /// It takes no input of its own. A press or a hover on it reaches what is
    /// under it, and a press there closes a popover it is outside of. Escape
    /// hides it until the next hover, drag or key, and goes on to whatever
    /// else wanted the key, the focused field included, as a plain tooltip's
    /// Escape does. Focus leaving the slider takes it down only when nothing
    /// else holds it up: the pointer still on the slider keeps it.
    ///
    /// The text is also the slider's accessible value, published beside the
    /// number: UI Automation and macOS carry it, so a reader there says
    /// "-3.0 dB" rather than "-3". AT-SPI carries the number alone, so Orca
    /// keeps reading the number.
    ///
    /// `format` receives the value the bound signal holds, on every change to
    /// it. It returns a [`LocalizedString`], so a `tr!(..)` message
    /// re-resolves on a locale change and `lit!(..)` serves text that is not
    /// translated.
    ///
    /// A [`tooltip`](Self::tooltip), rich or composite tooltip set beside it
    /// keeps its hover delay, its place under the slider and the accessible
    /// description it gives, and both show; to name the control in the
    /// readout instead, say so in `format`.
    pub fn value_tooltip(mut self, format: impl Fn(f32) -> LocalizedString + 'static) -> Self {
        self.value_format = Some(Rc::new(format));
        self
    }

    /// Let the user put the value back to `value`: with a double-click on the
    /// slider, or with the "Reset to default" accessibility action, which this
    /// also advertises.
    ///
    /// The reset is a user change, written through the same path as a drag or
    /// an arrow key, so [`on_change`](Self::on_change) reports it unless the
    /// value was already there. It is clamped to the range and not snapped to
    /// the [`step`](Self::step): a default between two steps lands where it
    /// was asked to. A double-click's first click jumps the value to where it
    /// landed, as every click on the track does, so `on_change` reports that
    /// value and then the default; further clicks in the same burst leave the
    /// default where it is. A disabled slider ignores the double-click, and
    /// neither offers nor takes the action.
    ///
    /// An application's own `.access_custom_action(..)` on the slider is
    /// offered beside the reset, not instead of it.
    pub fn default_value(mut self, value: f32) -> Self {
        self.default_value = Some(value);
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
        // The reset action is offered only while it would do something.
        effective_enabled.bind_to(
            self_id,
            ctx.binding_registry(),
            BindingLevel::AccessibilityOnly,
        );
        self.effective_enabled = Some(effective_enabled.clone());
        // `accessibility()` reads the value only when the tree is walked, and
        // the style's repaint does not walk it; see `Checkbox::build`.
        self.value.bind_to(
            self_id,
            ctx.binding_registry(),
            BindingLevel::AccessibilityOnly,
        );

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

        // One reporter for every user-driven write. It compares against the
        // value before the write, so a write that changes nothing — a drag
        // past the end, a snap landing on the grid point it was already on —
        // reports nothing, and a drag reports once per value it actually
        // produces rather than once per pointer sample.
        let report = {
            let value = value.clone();
            let on_change = self.on_change.clone();
            move |before: f32, ctx: &mut teksilo_core::widget::EventContext| {
                let now = value.get();
                if now != before
                    && let Some(ref f) = on_change
                {
                    f(now, ctx);
                }
            }
        };

        // The value readout, when one is configured, and the one call every
        // handler below ends with: it raises, moves or takes down the readout
        // after whatever the handler changed. A no-op on a slider without one.
        let readout = self.value_format.clone().map(|format| {
            Readout::mount(
                ctx,
                ReadoutInputs {
                    value: value.clone(),
                    min,
                    max,
                    orientation,
                    thumb_diameter: thumb_radius * 2.0,
                    bounds: cached_bounds.clone(),
                    hovered: hovered.clone(),
                    dragging: dragging.clone(),
                    focused: focused.clone(),
                },
                format,
                effective_enabled.clone(),
            )
        });
        let sync_readout = move |ctx: &mut teksilo_core::widget::EventContext| {
            if let Some(readout) = &readout {
                readout.sync(ctx);
            }
        };

        // The reset `default_value` asks for, written and reported like any
        // other user change. `None` when no default is set, and then neither a
        // double-click nor the reset action does anything of its own.
        let reset_to_default = self.default_value.map(|default| {
            let value = value.clone();
            let report = report.clone();
            let sync_readout = sync_readout.clone();
            move |ctx: &mut teksilo_core::widget::EventContext| {
                let before = value.get();
                value.set(default.clamp(min, max));
                report(before, ctx);
                sync_readout(ctx);
            }
        });
        // The clicks in a row the track has taken, for the double-click.
        let taps = Rc::new(Cell::new(TapStreak::EMPTY));

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

        // The pointer driving the drag, so a cancel can be told apart from one
        // aimed at some other contact.
        let drag_pointer: Rc<Cell<Option<PointerId>>> = Rc::new(Cell::new(None));

        // Thumb drag — routed through the typed gesture API.
        {
            let dragging = dragging.clone();
            let drag_pointer = drag_pointer.clone();
            let set_value = set_value_from_position.clone();
            let value_before = value.clone();
            let report = report.clone();
            let sync_readout = sync_readout.clone();
            let taps = taps.clone();
            handlers = handlers.on_drag(move |phase, ctx| {
                match phase {
                    DragPhase::Started {
                        position,
                        button: PointerButton::Primary,
                        pointer,
                    } => {
                        // A drag between two clicks makes them two clicks, not
                        // a double-click: the tap streak's own rule.
                        taps.set(TapStreak::EMPTY);
                        dragging.set(true);
                        drag_pointer.set(Some(pointer.id));
                        let before = value_before.get();
                        set_value(position.x, position.y, ctx.is_rtl());
                        report(before, ctx);
                    }
                    DragPhase::Moved { position, .. } if dragging.get() => {
                        let before = value_before.get();
                        set_value(position.x, position.y, ctx.is_rtl());
                        report(before, ctx);
                    }
                    // `Cancelled` keeps the gesture contract (one of the two
                    // follows every `Started`), though the tree today revokes a
                    // press as a `PointerCancel`, handled below.
                    DragPhase::Ended { .. } | DragPhase::Cancelled { .. } => {
                        dragging.set(false);
                        drag_pointer.set(None);
                    }
                    _ => return,
                }
                sync_readout(ctx);
            });
        }

        // A cancelled press is over too, with no `Ended` to say so (a modal
        // opening, the window losing focus). Left set, `dragging` kept the
        // pressed look, and now a readout, on for good.
        //
        // Only the dragging pointer's cancel ends the drag. The node hears
        // cancels for other contacts too — a second finger that landed on it
        // and was then taken away — and clearing the drag for one of those
        // froze the thumb under the finger still moving it.
        {
            let dragging = dragging.clone();
            let sync_readout = sync_readout.clone();
            handlers = handlers.on_pointer_cancel(move |pointer, _reason, ctx| {
                if drag_pointer.get() != Some(pointer.id) {
                    return;
                }
                drag_pointer.set(None);
                dragging.set(false);
                sync_readout(ctx);
            });
        }

        // Track click — jump the value to the click position — and, with a
        // `default_value`, the double-click that resets it.
        //
        // The double-click is counted here, on the node's own tap streak,
        // rather than with `on_double_tap`: a multi-tap recognizer on a node
        // takes the single-tap one off it (`WidgetTree::ensure_gesture_arena`),
        // and the click that jumps the value with it. `TapStreak` is the
        // framework's own continuation rule (same button, within the profile's
        // multi-tap interval and slop), so the two cannot disagree on what a
        // double-click is. Its second click resets instead of jumping; the
        // first has already jumped, as any click does. Every click after the
        // second that the streak still continues is part of the same gesture,
        // and resets too (to where the value already is) rather than jumping.
        {
            let set_value = set_value_from_position.clone();
            let value_before = value.clone();
            let report = report.clone();
            let sync_readout = sync_readout.clone();
            let reset = reset_to_default.clone();
            let taps = taps.clone();
            // The profile the recognizers would read, as the theme stands at
            // build time; the thumb radius above is read the same way.
            let input = ctx.theme().input;
            handlers = handlers.on_tap(move |event, ctx| {
                if let Some(ref reset) = reset {
                    // The streak measures from where each click was pressed;
                    // a tap's release is within its slop of that.
                    let profile = input.profile(event.pointer.kind);
                    let mut streak = taps.get();
                    let count =
                        streak.advance(event.pointer.time, profile, event.position, event.button);
                    if count >= 2 {
                        // Kept at one, as if this click had started the
                        // streak, so the next click that continues it counts
                        // two again. `TapStreak` counts to three and then starts
                        // over at one, which made the third click of a burst a
                        // jump and the fourth a fresh first click.
                        let mut rebased = TapStreak::EMPTY;
                        rebased.advance(event.pointer.time, profile, event.position, event.button);
                        taps.set(rebased);
                        reset(ctx);
                        return;
                    }
                    taps.set(streak);
                }
                let before = value_before.get();
                set_value(event.position.x, event.position.y, ctx.is_rtl());
                report(before, ctx);
                sync_readout(ctx);
            });
        }

        // Hover. Two writers for one flag, because neither is enough alone.
        // The tree's `hover_within` follows the pointer whatever the slider can
        // be told: a disabled slider hears no `PointerLeave`, and a parked one
        // has its hover cleared with no event at all, and either way a flag
        // kept by the handler alone stayed `true` and held the readout up for
        // whatever came next to raise it. The pointer is always over a strict
        // descendant here, the body the slider lays out over its whole bounds.
        // The handler is the other writer, because it is the one that runs with
        // an `EventContext` to raise the readout, and it runs before the tree
        // updates `hover_within` for the same move.
        handlers = handlers.hover_within(hovered.clone());
        {
            let hovered = hovered.clone();
            let sync_readout = sync_readout.clone();
            handlers = handlers.on_hover(move |entered, ctx| {
                hovered.set(entered);
                sync_readout(ctx);
            });
        }

        // Key handler. The chord table is shared with every other bounded
        // scalar (`common::range_nav`); what stays here is the arithmetic.
        {
            let adjust = adjust_by_step.clone();
            let value = value.clone();
            let report = report.clone();
            let sync_readout = sync_readout.clone();
            handlers = handlers.on_key(move |event, ctx| {
                let WidgetEvent::KeyDown { key, modifiers, .. } = event else {
                    return EventResponse::Ignored;
                };
                // The paint, the pointer mapping and the arrows all mirror
                // against this same answer, read at event time so a locale
                // flip needs no rebuild. Mirroring any one of the three alone
                // would leave the `Left` key and a leftward drag moving the
                // thumb in opposite directions.
                let response = match range_nav::range_move(
                    *key,
                    *modifiers,
                    RangeKind::Scalar,
                    RangeAxis::Both,
                    ctx.is_rtl(),
                ) {
                    Some(mv) => {
                        let before = value.get();
                        match mv {
                            RangeMove::Step { increase } => adjust(increase, false),
                            RangeMove::Page { increase } => adjust(increase, true),
                            RangeMove::ToMin => value.set(min),
                            RangeMove::ToMax => value.set(max),
                        }
                        report(before, ctx);
                        EventResponse::Handled
                    }
                    None => EventResponse::Ignored,
                };
                // After a key the slider does not take as well: any key makes
                // the focus keyboard focus, which the readout follows as the
                // focus ring does. Except Escape, which is how the readout is
                // taken down: the tree has already retired it by the time the
                // key gets here, and syncing would raise it straight back.
                if *key != Key::Escape {
                    sync_readout(ctx);
                }
                response
            });
        }

        // Focus handler. Track raw focus only; the keyboard/pointer
        // distinction is derived live from the input-modality signal in
        // `build()` (`:focus-visible`), so clicking to focus then pressing a
        // key reveals the ring.
        {
            let focused = focused.clone();
            let sync_readout = sync_readout.clone();
            handlers = handlers.on_focus(move |gained, ctx| {
                focused.set(gained);
                sync_readout(ctx);
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
            let value_before = value.clone();
            let report = report.clone();
            let reset = reset_to_default.clone();
            let sync_readout = sync_readout.clone();
            handlers = handlers.on_access_action_request(move |action, _node, data, ctx| {
                use teksilo_core::accesskit::{Action, ActionData};
                // "Reset to default", which reports for itself.
                if action == Action::CustomAction
                    && matches!(data, Some(ActionData::CustomAction(RESET_ACTION_ID)))
                    && let Some(ref reset) = reset
                {
                    reset(ctx);
                    return EventResponse::Handled;
                }
                let before = value_before.get();
                let outcome = match (action, data) {
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
                    // sends only strings. A number only: the `value_tooltip`
                    // text the node publishes ("-3.0 dB") has no inverse, so a
                    // write of it is declined rather than guessed at.
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
                };
                if outcome == EventResponse::Handled {
                    report(before, ctx);
                    sync_readout(ctx);
                }
                outcome
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
        regions.push(TargetRegion::grab(
            thumb_rect(
                bounds,
                fraction(self.value.get(), self.min, self.max),
                diameter,
                self.orientation,
                self.cached_rtl.get(),
            ),
            SLIDER_PART_THUMB,
        ));
        regions
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::Slider);
        if let Some(ref label) = self.label {
            builder.set_name(label.resolve_now());
        }
        let step = self.effective_step();
        builder.set_numeric_value(on_grid(self.value.get(), self.min, step));
        builder.set_min_numeric_value(as_written(self.min));
        builder.set_max_numeric_value(as_written(self.max));
        // Publish both keyboard distances so Orca / VoiceOver can announce
        // "step by N" for an arrow and the coarser figure for a page key.
        // Both come from the same helpers the handlers use, so the announced
        // number and the applied one cannot drift.
        builder.set_numeric_value_step(on_grid(step, self.min, step));
        builder.set_numeric_value_jump(on_grid(self.effective_page_step(), self.min, step));
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
        // The readout's text, beside the number: the string a reader that
        // speaks a value's text says, and the one the sighted user sees.
        if let Some(ref format) = self.value_format {
            builder.set_value(format(self.value.get()).resolve_now());
        }
        // Not on a disabled slider, where the action would be refused: an
        // assistive client offering it would offer a menu entry that does
        // nothing.
        let enabled = self.effective_enabled.as_ref().is_none_or(|on| on.get());
        if self.default_value.is_some() && enabled {
            builder.add_action(teksilo_core::accesskit::Action::CustomAction);
            builder.set_custom_actions(vec![teksilo_core::accesskit::CustomAction {
                id: RESET_ACTION_ID,
                description: teksilo_i18n::tr_widget!(slider_reset_to_default()).resolve_now(),
            }]);
        }
    }
}

/// Where `value` sits in `min..=max`, from 0 at `min` to 1 at `max`, clamped.
/// 0 for a range too narrow to divide by.
fn fraction(value: f32, min: f32, max: f32) -> f32 {
    let range = max - min;
    if range.abs() < f32::EPSILON {
        0.0
    } else {
        ((value - min) / range).clamp(0.0, 1.0)
    }
}

/// The knob's square at fraction `t` of the range, inside `bounds`, as the
/// default style paints it: the centre travels the length less one diameter,
/// a horizontal slider's minimum is at the leading edge (the right one under
/// RTL), and a vertical slider's is at the bottom.
///
/// One answer for [`Slider::target_regions`] and the value readout, so the
/// grab region reported and the thumb the readout clears are the thumb that
/// was painted.
fn thumb_rect(bounds: Rect, t: f32, diameter: f32, orientation: Orientation, rtl: bool) -> Rect {
    let radius = diameter * 0.5;
    let (cx, cy) = match orientation {
        Orientation::Horizontal => {
            let usable = (bounds.width - diameter).max(0.0);
            let t = if rtl { 1.0 - t } else { t };
            (
                bounds.x + radius + usable * t,
                bounds.y + bounds.height * 0.5,
            )
        }
        Orientation::Vertical => {
            let usable = (bounds.height - diameter).max(0.0);
            (
                bounds.x + bounds.width * 0.5,
                bounds.y + radius + usable * (1.0 - t),
            )
        }
    };
    Rect::new(cx - radius, cy - radius, diameter, diameter)
}

/// The `f64` a platform is handed for one of the slider's `f32`s: the one
/// nearest the shortest decimal that reads back as that `f32`, rather than the
/// `f32`'s exact binary value. `0.3_f32 as f64` is 0.30000001192092896, and a
/// screen reader speaks the number it is given: Orca reads every digit of a
/// value below 1 (`orca/ax_value.py`, `get_current_value_text`), and AT-SPI
/// carries no value text to say it otherwise.
fn as_written(value: f32) -> f64 {
    value.to_string().parse().unwrap_or(f64::from(value))
}

/// [`as_written`] for a figure on the grid the arrows walk from `min` by
/// `step`, put back on that grid when `f32` arithmetic has only drifted off
/// it. An arrow adds an `f32` step to an `f32` value and rounds the sum to an
/// `f32`, so 0.65 + 0.01 is 0.65999997, the third step up from 0.3 is
/// 0.32999998, and ten steps of 0.01 are 0.099999994. Every point of the grid
/// is written with no more decimal places than `min` and `step` take, so a
/// figure within a thousandth of a step of such a decimal is published as
/// that decimal. Any other figure, such as a value the application set
/// between two steps, is published as it is.
fn on_grid(figure: f32, min: f32, step: f32) -> f64 {
    fn places(value: f32) -> usize {
        // `Display` never writes an `f32` with an exponent.
        value
            .to_string()
            .split_once('.')
            .map_or(0, |(_, fraction)| fraction.len())
    }
    let written = as_written(figure);
    let places = places(min).max(places(step));
    let rounded: f64 = format!("{written:.places$}").parse().unwrap_or(written);
    if (rounded - written).abs() <= as_written(step).abs() * 1e-3 {
        rounded
    } else {
        written
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
    fn on_change_reports_each_value_the_user_produces_and_nothing_else() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let value = Signal::new(50.0_f32);
        let seen: Rc<RefCell<Vec<f32>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = seen.clone();
        let mut tree = WidgetTree::new();
        let s = tree.add(
            Slider::new(value.clone(), 0.0, 100.0)
                .step(10.0)
                .on_change(move |now, _ctx| sink.borrow_mut().push(now)),
        );
        tree.layout(SizeProposal::exact(200.0, 60.0));
        tree.focus(s);

        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        tree.press_key(Key::ArrowLeft, Modifiers::NONE);
        tree.press_key(Key::End, Modifiers::NONE);
        // Already at the maximum: the write clamps to the value it already
        // holds, which is not a change and is not reported.
        tree.press_key(Key::ArrowRight, Modifiers::NONE);

        assert_eq!(*seen.borrow(), vec![60.0, 50.0, 100.0]);
    }

    #[test]
    fn on_change_is_silent_for_a_programmatic_write() {
        use std::cell::Cell;
        use std::rc::Rc;

        let value = Signal::new(50.0_f32);
        let fired = Rc::new(Cell::new(false));
        let sink = fired.clone();
        let mut tree = WidgetTree::new();
        let _s = tree.add(
            Slider::new(value.clone(), 0.0, 100.0)
                .step(10.0)
                .on_change(move |_now, _ctx| sink.set(true)),
        );
        tree.layout(SizeProposal::exact(200.0, 60.0));

        value.set(70.0);
        assert!(!fired.get());
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
    fn a_reader_is_told_each_new_value_as_it_happens() {
        // An arrow moved the value and told no platform: the new figure came
        // out only with the next unrelated walk of the tree, most often the
        // next focus move, where Orca's "52" was cut by the new focus.
        use crate::common::heard_test::{Heard, Listener};

        let value = Signal::new(50.0_f32);
        let mut tree = WidgetTree::new();
        let s =
            tree.add(Slider::new(value.clone(), 0.0, 100.0).label(teksilo_i18n::lit!("Volume")));
        let p = SizeProposal::exact(200.0, 60.0);
        tree.layout(p);
        tree.focus(s);
        tree.layout(p);
        let mut listener = Listener::attach(&mut tree);

        for heard in ["51", "52"] {
            tree.press_key(Key::ArrowRight, Modifiers::NONE);
            tree.layout(p);
            assert_eq!(
                listener.heard(&mut tree),
                vec![Heard::FocusNumber(heard.to_string())],
                "Right"
            );
        }
    }

    #[test]
    fn a_reader_hears_the_figure_the_app_holds_not_its_binary_noise() {
        // An `f32` widened as it is reads 0.3 as 0.30000001192092896, and Orca
        // speaks every digit of a value under 1 (`ax_value.py`,
        // `get_current_value_text`).
        use crate::common::heard_test::{Heard, Listener};

        let value = Signal::new(0.3_f32);
        let mut tree = WidgetTree::new();
        let s = tree.add(
            Slider::new(value.clone(), 0.0, 1.0)
                .step(0.01)
                .orientation(Orientation::Vertical)
                .label(teksilo_i18n::lit!("Opacity")),
        );
        let p = SizeProposal::exact(60.0, 200.0);
        tree.layout(p);

        let update = tree.sync_accessibility();
        let nid = teksilo_core::accessibility::widget_id_to_node_id(s);
        let node = &update
            .nodes
            .iter()
            .find(|(id, _)| *id == nid)
            .expect("the slider publishes an AT node")
            .1;
        assert_eq!(
            (node.numeric_value(), node.numeric_value_step()),
            (Some(0.3), Some(0.01)),
        );

        tree.focus(s);
        tree.layout(p);
        let mut listener = Listener::attach(&mut tree);
        tree.press_key(Key::ArrowUp, Modifiers::NONE);
        tree.layout(p);
        assert_eq!(
            listener.heard(&mut tree),
            vec![Heard::FocusNumber("0.31".to_string())],
        );
    }

    #[test]
    fn a_reader_hears_each_step_on_its_grid_not_the_drift_of_f32_sums() {
        // Each arrow adds an `f32` step to an `f32` value and rounds the sum
        // to an `f32`, so the value drifts off the step's grid: 0.65 + 0.01 is
        // 0.65999997, the third step up from 0.3 is 0.32999998, and ten steps
        // of 0.01 are 0.099999994. Orca speaks every digit it is given below 1.
        use crate::common::heard_test::{Heard, Listener};

        // No step: the arrows walk 1 % of the range, as the menus example's
        // Opacity slider does from 0.65.
        let value = Signal::new(0.65_f32);
        let mut tree = WidgetTree::new();
        let s = tree.add(Slider::new(value.clone(), 0.0, 1.0).label(teksilo_i18n::lit!("Opacity")));
        let p = SizeProposal::exact(200.0, 60.0);
        tree.layout(p);
        assert_eq!(
            a11y_steps(&mut tree, s),
            (Some(0.01), Some(0.1)),
            "the arrow and page distances"
        );
        tree.focus(s);
        tree.layout(p);
        let mut listener = Listener::attach(&mut tree);
        for heard in ["0.66", "0.67", "0.68"] {
            tree.press_key(Key::ArrowRight, Modifiers::NONE);
            tree.layout(p);
            assert_eq!(
                listener.heard(&mut tree),
                vec![Heard::FocusNumber(heard.to_string())],
                "Right"
            );
        }

        value.set(0.3);
        tree.layout(p);
        let _ = listener.heard(&mut tree);
        for heard in ["0.31", "0.32", "0.33"] {
            tree.press_key(Key::ArrowRight, Modifiers::NONE);
            tree.layout(p);
            assert_eq!(
                listener.heard(&mut tree),
                vec![Heard::FocusNumber(heard.to_string())],
                "Right from 0.3"
            );
        }

        // A value the app set between two steps is not its neighbour on the
        // grid, and is published as it is.
        value.set(0.655);
        tree.layout(p);
        assert_eq!(
            listener.heard(&mut tree),
            vec![Heard::FocusNumber("0.655".to_string())],
            "a value off the grid"
        );
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
        // `place_children` cached the bounds for event handling during the
        // layout above; the render is what paints the body.
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
                .child(sid),
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
                .child(slider)
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
        let _surface = tree.add(crate::primitives::ZStack::new().child(slider).on_pinch(
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

    /// A vertical slider reports its knob where the body paints it: its
    /// minimum is at the **bottom**.
    ///
    /// The report ran the other way, from the top, while the paint and the
    /// pointer mapping both put the maximum at the top, so a quarter-value
    /// knob was reported three quarters of the way down.
    #[test]
    fn a_vertical_slider_reports_its_knob_where_it_paints_it() {
        use crate::styles::recipe_slider_style::SLIDER_THUMB_DIAMETER;

        let theme = teksilo_core::presets::intui::light();
        let accent = theme.colors.accent.to_array();
        let mut tree = WidgetTree::new().with_theme(theme);
        let s = tree
            .add(Slider::new(Signal::new(25.0_f32), 0.0, 100.0).orientation(Orientation::Vertical));
        tree.layout(SizeProposal::exact(60.0, 200.0));
        let painted = tree
            .render()
            .shapes
            .iter()
            .find(|q| {
                q.color == accent
                    && (q.screen[2] - SLIDER_THUMB_DIAMETER).abs() < 0.01
                    && (q.screen[3] - SLIDER_THUMB_DIAMETER).abs() < 0.01
            })
            .map(|q| q.screen[1] + q.screen[3] * 0.5)
            .expect("the body paints a knob");
        let reported = tree
            .widget_target_regions(s)
            .into_iter()
            .find(|r| r.part == SLIDER_PART_THUMB)
            .expect("the slider reports its knob")
            .rect
            .center()
            .y;
        let bounds = tree.bounds(s);
        assert!(
            painted > bounds.center().y,
            "a quarter-value knob is painted in the lower half ({painted})"
        );
        assert!(
            (reported - painted).abs() < 0.51,
            "the knob is reported at y {reported} and painted at y {painted}",
        );
    }

    // -----------------------------------------------------------------
    // The value readout and the reset to a default
    // -----------------------------------------------------------------

    use crate::primitives::{FixedSize, HStack, VStack};
    use std::cell::RefCell;
    use teksilo_i18n::lit;

    /// The readout an equalizer band shows: the gain in decibels, signed.
    fn db(gain: f32) -> LocalizedString {
        lit!(format!("{gain:+.1} dB"))
    }

    /// The window every readout test runs in.
    fn window() -> SizeProposal {
        SizeProposal::exact(400.0, 300.0)
    }

    /// A tree with real-looking text metrics, so a readout has a size to
    /// place.
    fn readout_tree() -> WidgetTree {
        WidgetTree::new()
            .with_theme(teksilo_core::presets::intui::light())
            .with_text_backend(Rc::new(
                RefCell::new(teksilo_canvas::MockTextBackend::new()),
            ))
    }

    /// `slider`, full width, 100 dp below the top of the window, with room
    /// above it for the readout and below it to take the pointer to.
    fn horizontal_scene(slider: Slider) -> (WidgetTree, WidgetId) {
        let mut tree = readout_tree();
        let s = tree.add(slider);
        let _column = tree.add(
            VStack::new()
                .child(FixedSize::new().height(100.0))
                .child(s)
                .child(FixedSize::new().height(100.0)),
        );
        tree.layout(window());
        (tree, s)
    }

    /// Somewhere in the window the slider is not.
    fn away() -> Point {
        Point::new(200.0, 280.0)
    }

    /// The readout up over the slider, as its text and where it sits, or
    /// `None` while none is.
    ///
    /// The surface is the tooltip module's `TooltipWidget`, which keeps its
    /// text private; its `Debug` is the one place the text can be read from
    /// here.
    fn readout(tree: &WidgetTree) -> Option<(String, Rect)> {
        let overlays = tree.overlay_manager();
        overlays
            .active_content_ids()
            .into_iter()
            .find_map(|content| {
                let debug = tree.widget_debug_string(content)?;
                let text = debug
                    .split("TooltipWidget { text: \"")
                    .nth(1)?
                    .split('"')
                    .next()?
                    .to_owned();
                let bounds = tree.overlay_content_bounds(overlays.find_by_content(content)?)?;
                Some((text, bounds))
            })
    }

    /// The knob's rect, as the slider reports it.
    fn knob(tree: &WidgetTree, s: WidgetId) -> Rect {
        tree.widget_target_regions(s)
            .into_iter()
            .find(|r| r.part == SLIDER_PART_THUMB)
            .expect("the slider reports its knob")
            .rect
    }

    /// The slider's raw AccessKit node, read through `f`.
    fn at_node<R>(
        tree: &mut WidgetTree,
        s: WidgetId,
        f: impl FnOnce(&teksilo_core::accesskit::Node) -> R,
    ) -> R {
        let update = tree.sync_accessibility();
        let nid = teksilo_core::accessibility::widget_id_to_node_id(s);
        let node = &update
            .nodes
            .iter()
            .find(|(id, _)| *id == nid)
            .expect("the slider publishes an AT node")
            .1;
        f(node)
    }

    /// Two primary clicks at `at`, back to back.
    fn double_click(tree: &mut WidgetTree, at: Point) {
        tree.pointer_move(at);
        for _ in 0..2 {
            tree.pointer_down_button(at, PointerButton::Primary);
            tree.pointer_up_button(at, PointerButton::Primary);
        }
    }

    /// One primary click at `at`.
    fn click(tree: &mut WidgetTree, at: Point) {
        tree.pointer_move(at);
        tree.pointer_down_button(at, PointerButton::Primary);
        tree.pointer_up_button(at, PointerButton::Primary);
    }

    #[test]
    fn the_readout_is_up_at_once_while_the_pointer_is_over_the_slider() {
        let (mut tree, s) =
            horizontal_scene(Slider::new(Signal::new(-3.0_f32), -12.0, 12.0).value_tooltip(db));
        assert_eq!(
            readout(&tree),
            None,
            "nothing is up before the pointer comes"
        );

        tree.pointer_move(tree.bounds(s).center());
        tree.layout(window());
        let (text, _) = readout(&tree).expect("up with no hover delay");
        assert_eq!(text, "-3.0 dB");
        // The slider's own node carries the text as its value, so the surface
        // must not carry it a second time.
        let listener = crate::common::heard_test::Listener::attach(&mut tree);
        assert!(
            !listener.finds(teksilo_core::accesskit::Role::Tooltip, "-3.0 dB"),
            "the readout is hidden from assistive technology",
        );

        tree.pointer_move(away());
        tree.layout(window());
        assert_eq!(readout(&tree), None, "and gone when the pointer leaves");
    }

    #[test]
    fn the_readout_follows_the_value_through_a_drag() {
        let value = Signal::new(0.0_f32);
        let (mut tree, s) = horizontal_scene(
            Slider::new(value.clone(), -12.0, 12.0)
                .step(0.5)
                .value_tooltip(db),
        );
        let bounds = tree.bounds(s);
        let start = knob(&tree, s).center();
        tree.pointer_move(start);
        tree.pointer_down_button(start, PointerButton::Primary);
        tree.layout(window());
        let surface = tree.overlay_manager().active_content_ids();

        // Past the drag slop, then along the track, with the pointer taken off
        // the slider altogether: a drag carries on there, and so must the
        // readout.
        let mut last_x = f32::MIN;
        for dx in [10.0, 60.0, 120.0] {
            tree.pointer_move(Point::new(start.x + dx, bounds.bottom() + 40.0));
            tree.layout(window());
            let (text, rect) = readout(&tree).expect("up for the whole drag");
            assert_eq!(
                text,
                db(value.get()).resolve_now(),
                "the value as it stands"
            );
            let thumb = knob(&tree, s);
            assert!(
                (rect.center().x - thumb.center().x).abs() < 0.51,
                "centred on the thumb: {rect:?} over {thumb:?}",
            );
            assert!(rect.bottom() <= bounds.y, "above the slider: {rect:?}");
            assert!(rect.center().x >= last_x, "moving with the thumb");
            last_x = rect.center().x;
        }
        assert!(
            value.get() > 5.0,
            "the drag moved the value: {}",
            value.get()
        );
        assert_eq!(
            tree.overlay_manager().active_content_ids(),
            surface,
            "one surface, re-placed and re-worded, not a new one per value",
        );

        // The slider holds the hover while the drag holds the pointer; the
        // first motion after the release gives it back.
        let off = Point::new(start.x + 120.0, bounds.bottom() + 40.0);
        tree.pointer_up_button(off, PointerButton::Primary);
        tree.pointer_move(off);
        tree.layout(window());
        assert_eq!(
            readout(&tree),
            None,
            "the drag is over and the pointer is not on the slider"
        );
    }

    #[test]
    fn the_readout_follows_the_arrows_under_keyboard_focus() {
        let value = Signal::new(0.0_f32);
        let mut tree = readout_tree();
        let s = tree.add(
            Slider::new(value.clone(), -12.0, 12.0)
                .step(1.0)
                .value_tooltip(db),
        );
        let next = tree.add(crate::button::Button::new(lit!("Next")));
        let _column = tree.add(
            VStack::new()
                .child(FixedSize::new().height(100.0))
                .child(s)
                .child(next),
        );
        tree.layout(window());

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(s));
        tree.layout(window());
        let (text, mut at) = readout(&tree).expect("keyboard focus raises the readout");
        assert_eq!(text, "+0.0 dB");

        for said in ["+1.0 dB", "+2.0 dB"] {
            tree.press_key(Key::ArrowRight, Modifiers::NONE);
            tree.layout(window());
            let (text, rect) = readout(&tree).expect("still up");
            assert_eq!(text, said);
            assert!(
                rect.center().x > at.center().x,
                "it moved with the thumb: {rect:?} after {at:?}",
            );
            at = rect;
        }

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(next));
        tree.layout(window());
        assert_eq!(readout(&tree), None, "focus leaving takes it down");
    }

    #[test]
    fn a_clicked_slider_shows_its_readout_again_on_the_first_key() {
        // A click focuses the slider without making it keyboard focus, so the
        // readout goes with the pointer, as the focus ring stays hidden, and
        // comes back with the key that adjusts the value.
        let value = Signal::new(0.0_f32);
        let (mut tree, s) = horizontal_scene(
            Slider::new(value.clone(), -12.0, 12.0)
                .step(1.0)
                .value_tooltip(db),
        );
        let thumb = knob(&tree, s).center();
        tree.pointer_move(thumb);
        tree.pointer_down_button(thumb, PointerButton::Primary);
        tree.pointer_up_button(thumb, PointerButton::Primary);
        tree.pointer_move(away());
        tree.layout(window());
        assert_eq!(tree.focused(), Some(s), "the click focused the slider");
        assert_eq!(readout(&tree), None, "but not from the keyboard");

        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        tree.layout(window());
        assert_eq!(
            readout(&tree).map(|(text, _)| text).as_deref(),
            Some("+1.0 dB")
        );
    }

    #[test]
    fn escape_takes_the_readout_down_until_the_next_key() {
        // Content shown on focus has to be dismissible without moving focus
        // (WCAG 2.2 SC 1.4.13).
        let mut tree = readout_tree();
        let s = tree.add(
            Slider::new(Signal::new(0.0_f32), -12.0, 12.0)
                .step(1.0)
                .value_tooltip(db),
        );
        tree.layout(window());
        tree.press_key(Key::Tab, Modifiers::NONE);
        tree.layout(window());
        assert!(readout(&tree).is_some());

        tree.press_key(Key::Escape, Modifiers::NONE);
        tree.layout(window());
        assert_eq!(readout(&tree), None, "Escape dismisses it");
        assert_eq!(tree.focused(), Some(s), "and leaves focus where it was");

        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        tree.layout(window());
        assert!(readout(&tree).is_some(), "the next key brings it back");
    }

    #[test]
    fn a_horizontal_readout_goes_below_the_thumb_against_the_top_of_the_window() {
        let mut tree = readout_tree();
        let s = tree.add(Slider::new(Signal::new(0.0_f32), -12.0, 12.0).value_tooltip(db));
        let _column = tree.add(VStack::new().child(s).child(FixedSize::new().height(200.0)));
        tree.layout(window());
        let bounds = tree.bounds(s);
        assert!(bounds.y < 1.0, "the slider is at the top: {bounds:?}");

        tree.pointer_move(bounds.center());
        tree.layout(window());
        let (_, rect) = readout(&tree).expect("up");
        assert!(
            rect.y >= bounds.bottom(),
            "no room above, so below the slider: {rect:?} under {bounds:?}",
        );
    }

    #[test]
    fn a_vertical_readout_sits_beside_the_slider_and_rides_its_thumb() {
        let value = Signal::new(0.0_f32);
        let mut tree = readout_tree();
        let s = tree.add(
            Slider::new(value.clone(), -12.0, 12.0)
                .step(1.0)
                .orientation(Orientation::Vertical)
                .value_tooltip(db),
        );
        let _row = tree.add(
            HStack::new()
                .child(FixedSize::new().width(150.0))
                .child(FixedSize::new().height(200.0).child(s)),
        );
        tree.layout(window());
        let bounds = tree.bounds(s);

        tree.pointer_move(knob(&tree, s).center());
        tree.layout(window());
        let (_, low) = readout(&tree).expect("up");
        assert!(
            low.right() <= bounds.x,
            "on the inline-start side, clear of the slider: {low:?} beside {bounds:?}",
        );
        let thumb = knob(&tree, s);
        assert!(
            low.y < thumb.bottom() + 8.0 && low.bottom() > thumb.y,
            "level with the thumb rather than somewhere along the track: {low:?} by {thumb:?}",
        );

        // The pointer is still on the slider; the key moves the value under it.
        tree.focus(s);
        tree.press_key(Key::ArrowUp, Modifiers::NONE);
        tree.layout(window());
        let (_, high) = readout(&tree).expect("still up");
        assert!(
            high.y < low.y,
            "a higher value raises the thumb, and the readout with it: {high:?} after {low:?}",
        );
    }

    #[test]
    fn the_readout_text_is_the_accessible_value() {
        use crate::common::heard_test::{Heard, Listener};

        let value = Signal::new(-3.0_f32);
        let mut tree = WidgetTree::new();
        let s = tree.add(
            Slider::new(value.clone(), -12.0, 12.0)
                .step(0.5)
                .label(lit!("Low shelf"))
                .value_tooltip(db),
        );
        let p = SizeProposal::exact(200.0, 60.0);
        tree.layout(p);
        assert_eq!(
            at_node(&mut tree, s, |n| (
                n.value().map(str::to_owned),
                n.numeric_value()
            )),
            (Some("-3.0 dB".to_owned()), Some(-3.0)),
            "the text beside the number",
        );

        tree.focus(s);
        tree.layout(p);
        let mut listener = Listener::attach(&mut tree);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        tree.layout(p);
        assert_eq!(
            listener.heard(&mut tree),
            vec![Heard::FocusValue("-2.5 dB".to_owned())],
        );
    }

    #[test]
    fn a_double_click_resets_to_the_default_and_reports_it() {
        // Two clicks are a double-click by the mouse's own multi-tap rule, the
        // one `TapStreak` reads: within its interval, and within its slop, of
        // the click before. Each half of the rule is broken once below.
        let theme = teksilo_core::presets::intui::light();
        let mouse = theme.input.profile(teksilo_tokens::PointerKind::Mouse);
        let (interval, slop) = (mouse.multi_tap_interval, mouse.multi_tap_slop);
        let scene = || {
            let value = Signal::new(6.0_f32);
            let seen: Rc<RefCell<Vec<f32>>> = Rc::default();
            let sink = seen.clone();
            let (tree, s) = horizontal_scene(
                Slider::new(value.clone(), -12.0, 12.0)
                    .default_value(0.0)
                    .on_change(move |now, _ctx| sink.borrow_mut().push(now)),
            );
            let bounds = tree.bounds(s);
            let first = Point::new(bounds.x + bounds.width * 0.8, bounds.center().y);
            (tree, value, seen, first)
        };

        // A real one: the second click a little later than the first and a
        // little off it, inside both limits.
        let (mut tree, value, seen, first) = scene();
        click(&mut tree, first);
        tree.advance_time(interval / 2);
        click(&mut tree, Point::new(first.x + slop / 2.0, first.y));
        assert_eq!(value.get(), 0.0, "the double-click put the default back");
        let reported = seen.borrow().clone();
        assert_eq!(
            reported.len(),
            2,
            "the first click's jump, then the reset: {reported:?}"
        );
        assert_eq!(
            reported.last(),
            Some(&0.0),
            "the reset is reported, and last"
        );
        // Nothing is left pressed or dragging behind it.
        tree.assert_no_leaked_pointer_state();

        // Too slow: the second click is a click of its own, and jumps to
        // where the first one already put the value.
        let (mut tree, value, seen, first) = scene();
        click(&mut tree, first);
        let jumped = value.get();
        tree.advance_time(interval + std::time::Duration::from_millis(1));
        click(&mut tree, first);
        assert_eq!(value.get(), jumped, "two slow clicks are two clicks");
        assert_eq!(seen.borrow().len(), 1, "and nothing was reset");

        // Too far apart: the second click jumps to where it landed.
        let (mut tree, value, seen, first) = scene();
        click(&mut tree, first);
        let jumped = value.get();
        tree.advance_time(interval / 2);
        click(&mut tree, Point::new(first.x - slop * 4.0, first.y));
        assert!(
            value.get() < jumped && value.get() != 0.0,
            "a click outside the slop jumps rather than resets: {} after {jumped}",
            value.get()
        );
        assert!(
            seen.borrow().iter().all(|v| *v != 0.0),
            "and nothing was reset: {:?}",
            seen.borrow()
        );
    }

    #[test]
    fn an_applications_custom_action_keeps_the_reset_beside_it() {
        use teksilo_core::accesskit::{Action, ActionData};
        use teksilo_core::widget_builder::WidgetBuilder;

        let value = Signal::new(6.0_f32);
        let pinned = Rc::new(Cell::new(0_u32));
        let sink = pinned.clone();
        let mut tree = WidgetTree::new();
        let s = tree.add(
            Slider::new(value.clone(), -12.0, 12.0)
                .default_value(0.0)
                .access_custom_action(lit!("Pin band"), move |_ctx| sink.set(sink.get() + 1)),
        );
        tree.layout(SizeProposal::exact(200.0, 60.0));
        let offered = at_node(&mut tree, s, |n| {
            n.custom_actions()
                .iter()
                .map(|a| (a.id, a.description.clone()))
                .collect::<Vec<_>>()
        });
        assert_eq!(
            offered,
            vec![
                (RESET_ACTION_ID, "Reset to default".to_owned()),
                (0, "Pin band".to_owned()),
            ],
        );

        assert!(access(
            &mut tree,
            s,
            Action::CustomAction,
            Some(ActionData::CustomAction(RESET_ACTION_ID)),
        ));
        assert_eq!(
            (value.get(), pinned.get()),
            (0.0, 0),
            "the reset is the slider's"
        );
        value.set(6.0);
        assert!(access(
            &mut tree,
            s,
            Action::CustomAction,
            Some(ActionData::CustomAction(0)),
        ));
        assert_eq!(
            (value.get(), pinned.get()),
            (6.0, 1),
            "and the application's is its own"
        );
    }

    #[test]
    fn the_reset_action_resets_and_reports() {
        use teksilo_core::accesskit::{Action, ActionData};

        let value = Signal::new(6.0_f32);
        let seen: Rc<RefCell<Vec<f32>>> = Rc::default();
        let sink = seen.clone();
        let mut tree = WidgetTree::new();
        let s = tree.add(
            Slider::new(value.clone(), -12.0, 12.0)
                .default_value(0.0)
                .on_change(move |now, _ctx| sink.borrow_mut().push(now)),
        );
        tree.layout(SizeProposal::exact(200.0, 60.0));

        let (advertised, offered) = at_node(&mut tree, s, |n| {
            (
                n.supports_action(Action::CustomAction),
                n.custom_actions()
                    .iter()
                    .map(|a| (a.id, a.description.clone()))
                    .collect::<Vec<_>>(),
            )
        });
        assert!(
            advertised,
            "the custom action is reachable only when advertised"
        );
        assert_eq!(
            offered,
            vec![(RESET_ACTION_ID, "Reset to default".to_owned())]
        );

        assert!(access(
            &mut tree,
            s,
            Action::CustomAction,
            Some(ActionData::CustomAction(RESET_ACTION_ID)),
        ));
        assert_eq!(value.get(), 0.0);
        assert_eq!(*seen.borrow(), vec![0.0], "reported as a user change");
    }

    #[test]
    fn the_reset_action_is_named_in_the_users_language() {
        // From the widget catalogue, and resolved at each walk, so a locale
        // switch renames it.
        use crate::common::locale_switch_test::speaking;

        let (manager, mut tree) = speaking("en-US");
        let s = tree.add(Slider::new(Signal::new(6.0_f32), -12.0, 12.0).default_value(0.0));
        let p = SizeProposal::exact(200.0, 60.0);
        tree.layout(p);
        let name = |tree: &mut WidgetTree| {
            at_node(tree, s, |n| {
                n.custom_actions()
                    .iter()
                    .find(|a| a.id == RESET_ACTION_ID)
                    .map(|a| a.description.clone())
            })
        };
        assert_eq!(name(&mut tree).as_deref(), Some("Reset to default"));

        manager.set_locale("fr-FR".parse().unwrap());
        tree.set_locale("fr-FR".to_string());
        tree.layout(p);
        assert_eq!(
            name(&mut tree).as_deref(),
            Some("Rétablir la valeur par défaut")
        );
        teksilo_i18n::thread_local::clear();
    }

    #[test]
    fn a_disabled_slider_neither_resets_nor_shows_a_readout() {
        use teksilo_core::accesskit::{Action, ActionData};

        let value = Signal::new(6.0_f32);
        let seen: Rc<RefCell<Vec<f32>>> = Rc::default();
        let sink = seen.clone();
        let (mut tree, s) = horizontal_scene(
            Slider::new(value.clone(), -12.0, 12.0)
                .enabled(false)
                .default_value(0.0)
                .value_tooltip(db)
                .on_change(move |now, _ctx| sink.borrow_mut().push(now)),
        );
        let bounds = tree.bounds(s);

        tree.pointer_move(bounds.center());
        tree.layout(window());
        assert_eq!(readout(&tree), None, "no readout over a disabled slider");

        double_click(
            &mut tree,
            Point::new(bounds.x + bounds.width * 0.8, bounds.center().y),
        );
        let _ = access(
            &mut tree,
            s,
            Action::CustomAction,
            Some(ActionData::CustomAction(RESET_ACTION_ID)),
        );
        assert_eq!(
            value.get(),
            6.0,
            "neither the double-click nor the action reset it"
        );
        assert!(seen.borrow().is_empty(), "and nothing was reported");
    }

    #[test]
    fn disabling_a_slider_under_the_pointer_takes_its_readout_down() {
        // No slider handler runs when the application disables it, so this is
        // the surface's own gate at work.
        let enabled = Signal::new(true);
        let (mut tree, s) = horizontal_scene(
            Slider::new(Signal::new(0.0_f32), -12.0, 12.0)
                .enabled(enabled.clone())
                .value_tooltip(db),
        );
        tree.pointer_move(tree.bounds(s).center());
        tree.layout(window());
        assert!(readout(&tree).is_some());

        enabled.set(false);
        tree.layout(window());
        tree.layout(window());
        assert_eq!(readout(&tree), None, "disabled under the pointer");
        assert!(
            tree.active_overlays().is_empty(),
            "with no overlay left behind"
        );

        enabled.set(true);
        tree.layout(window());
        assert!(
            tree.active_overlays().is_empty(),
            "re-enabling raises nothing until the next hover, drag or key",
        );
    }

    /// The `on_pointer_cancel` path, and only that one: the tree takes a
    /// revoked press back by cancelling the recognizers silently and then
    /// delivering a `PointerCancel` for the pointer, so `on_drag` never hears a
    /// `Cancelled` here. With the dragging pointer's cancel ignored this fails;
    /// with the `DragPhase::Cancelled` arm removed it passes. Its twin,
    /// `another_contacts_cancel_leaves_a_live_drag_running`, holds the same
    /// handler to the dragging pointer alone.
    #[test]
    fn a_cancelled_touch_drag_leaves_no_readout() {
        // A contact never hovers, so the drag alone held the readout up.
        let value = Signal::new(0.0_f32);
        let (mut tree, s) =
            horizontal_scene(Slider::new(value.clone(), -12.0, 12.0).value_tooltip(db));
        let start = knob(&tree, s).center();
        let finger = tree.new_contact();
        tree.touch_down(finger, start);
        for dx in [30.0, 60.0] {
            tree.touch_move(finger, Point::new(start.x + dx, start.y));
        }
        tree.layout(window());
        assert!(
            readout(&tree).is_some(),
            "a finger's drag shows the readout"
        );

        tree.touch_cancel(finger, Point::new(start.x + 60.0, start.y));
        tree.layout(window());
        assert_eq!(readout(&tree), None, "and a cancel ends the drag");
        tree.assert_no_leaked_pointer_state();
    }

    #[test]
    fn a_slider_without_a_readout_or_a_default_is_as_it_was() {
        let value = Signal::new(50.0_f32);
        let (mut tree, s) = horizontal_scene(Slider::new(value.clone(), 0.0, 100.0));
        let bounds = tree.bounds(s);

        tree.pointer_move(bounds.center());
        tree.focus(s);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        tree.layout(window());
        assert!(
            tree.active_overlays().is_empty(),
            "no overlay on hover or key"
        );
        let (text, custom) = at_node(&mut tree, s, |n| {
            (
                n.value().map(str::to_owned),
                n.supports_action(teksilo_core::accesskit::Action::CustomAction),
            )
        });
        assert_eq!(text, None, "no value text: a reader hears the number");
        assert!(!custom, "and no reset action");

        // A double-click is two clicks on the track, and the value stays where
        // they landed.
        double_click(
            &mut tree,
            Point::new(bounds.x + bounds.width * 0.75, bounds.center().y),
        );
        assert!(
            (value.get() - 75.0).abs() < 2.0,
            "two clicks at three quarters leave the value there: {}",
            value.get()
        );
    }

    // -----------------------------------------------------------------
    // What the readout must not do to the rest of the window
    // -----------------------------------------------------------------

    /// `slider` in a column, with `above` and `below` around it and room for
    /// the readout above them.
    fn between(
        above: WidgetId,
        slider: Slider,
        below: WidgetId,
        tree: &mut WidgetTree,
    ) -> WidgetId {
        let s = tree.add(slider);
        let _column = tree.add(
            VStack::new()
                .child(FixedSize::new().height(100.0))
                .child(above)
                .child(s)
                .child(below)
                .child(FixedSize::new().height(60.0)),
        );
        tree.layout(window());
        s
    }

    #[test]
    fn escape_leaves_focus_on_the_slider_the_click_focused() {
        // The readout never takes focus, so taking it down has none to give
        // back: not to the field that held focus when the pointer arrived.
        let mut tree = readout_tree();
        let field = tree.add(crate::button::Button::new(lit!("Field")));
        let next = tree.add(crate::button::Button::new(lit!("Next")));
        let s = between(
            field,
            Slider::new(Signal::new(0.0_f32), -12.0, 12.0).value_tooltip(db),
            next,
            &mut tree,
        );
        tree.focus(field);

        let thumb = knob(&tree, s).center();
        tree.pointer_move(thumb);
        tree.layout(window());
        assert!(readout(&tree).is_some(), "the pointer raised it");
        tree.pointer_down_button(thumb, PointerButton::Primary);
        tree.pointer_up_button(thumb, PointerButton::Primary);
        tree.layout(window());
        assert_eq!(tree.focused(), Some(s), "the click focused the slider");
        assert!(readout(&tree).is_some(), "and the pointer keeps it up");

        tree.press_key(Key::Escape, Modifiers::NONE);
        tree.layout(window());
        assert_eq!(readout(&tree), None, "Escape took it down");
        assert_eq!(
            tree.focused(),
            Some(s),
            "and left focus where the click put it"
        );
    }

    #[test]
    fn a_press_and_a_hover_on_the_readout_reach_what_is_under_it() {
        use teksilo_core::widget_builder::WidgetBuilder;

        let taps = Rc::new(Cell::new(0_u32));
        let count = taps.clone();
        let mut tree = readout_tree();
        let under = tree.add(
            FixedSize::new()
                .width(400.0)
                .height(40.0)
                .on_tap(move |_e, _c| count.set(count.get() + 1)),
        );
        let below = tree.add(FixedSize::new().height(10.0));
        let s = between(
            under,
            Slider::new(Signal::new(0.0_f32), -12.0, 12.0)
                .step(1.0)
                .value_tooltip(db),
            below,
            &mut tree,
        );
        tree.focus(s);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        tree.layout(window());
        let (_, bubble) = readout(&tree).expect("up from the keyboard");
        let at = bubble.center();
        assert!(
            tree.bounds(under).contains(at),
            "fixture: the bubble hangs over the target, {bubble:?} over {:?}",
            tree.bounds(under),
        );

        tree.pointer_move(at);
        assert_eq!(tree.hovered(), Some(under), "the hover is the target's");
        tree.pointer_down_button(at, PointerButton::Primary);
        tree.pointer_up_button(at, PointerButton::Primary);
        assert_eq!(taps.get(), 1, "and so is the press");
    }

    #[test]
    fn a_press_on_the_readout_still_closes_a_popover_it_is_outside_of() {
        use teksilo_core::overlay::{
            DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest,
        };

        let mut tree = readout_tree();
        let above = tree.add(FixedSize::new().height(40.0));
        let trigger = tree.add(FixedSize::new().width(60.0).height(20.0));
        let s = between(
            above,
            Slider::new(Signal::new(0.0_f32), -12.0, 12.0)
                .step(1.0)
                .value_tooltip(db),
            trigger,
            &mut tree,
        );
        // Below its trigger, which is below the slider: nowhere near the
        // bubble, which goes above the slider.
        let panel = tree.add(FixedSize::new().width(120.0).height(30.0));
        tree.show_overlay(OverlayRequest {
            content_id: panel,
            anchor: trigger,
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::ClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        tree.layout(window());
        // Raised after the popover, so it sits above it on the stack.
        tree.focus(s);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        tree.layout(window());
        let (_, bubble) = readout(&tree).expect("up from the keyboard");
        let popover = tree
            .overlay_manager()
            .find_by_content(panel)
            .expect("the popover is up");
        let popover_bounds = tree.overlay_content_bounds(popover).expect("and laid out");
        assert!(
            !popover_bounds.contains(bubble.center()),
            "fixture: the press lands outside the popover"
        );

        tree.pointer_down_button(bubble.center(), PointerButton::Primary);
        tree.pointer_up_button(bubble.center(), PointerButton::Primary);
        tree.layout(window());
        assert_eq!(
            tree.overlay_manager().find_by_content(panel),
            None,
            "a press outside the popover closes it, bubble or no bubble",
        );
    }

    #[test]
    fn a_readout_raised_by_tab_follows_the_scroll_that_reveals_the_slider() {
        let mut tree = readout_tree();
        let s = tree.add(Slider::new(Signal::new(0.0_f32), -12.0, 12.0).value_tooltip(db));
        let page = tree.add(
            VStack::new()
                .child(FixedSize::new().height(600.0))
                .child(s)
                .child(FixedSize::new().height(60.0)),
        );
        let _scroller = tree.add(crate::scroll_area::ScrollArea::new().child(page));
        tree.layout(window());
        assert!(
            tree.bounds(s).y > 300.0,
            "fixture: the slider starts below the fold"
        );

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(s));
        tree.layout(window());
        tree.layout(window());
        let slider = tree.bounds(s);
        assert!(
            slider.bottom() <= 300.0,
            "fixture: Tab scrolled it into view, {slider:?}"
        );
        let (_, bubble) = readout(&tree).expect("Tab raised it");
        let thumb = knob(&tree, s);
        assert!(
            (bubble.center().x - thumb.center().x).abs() < 0.51,
            "centred on the thumb: {bubble:?} over {thumb:?}",
        );
        assert!(
            bubble.bottom() <= slider.y && bubble.bottom() > slider.y - 24.0,
            "just above the slider where the scroll put it: {bubble:?} over {slider:?}",
        );
    }

    #[test]
    fn the_readout_follows_the_thumb_when_the_window_resizes() {
        let value = Signal::new(6.0_f32);
        let (mut tree, s) = horizontal_scene(
            Slider::new(value.clone(), -12.0, 12.0)
                .step(1.0)
                .value_tooltip(db),
        );
        tree.focus(s);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        tree.layout(window());
        let (_, wide) = readout(&tree).expect("up from the keyboard");

        let narrow = SizeProposal::exact(250.0, 300.0);
        tree.layout(narrow);
        tree.layout(narrow);
        let (_, after) = readout(&tree).expect("still up");
        let thumb = knob(&tree, s);
        assert!(
            after.center().x < wide.center().x - 50.0,
            "fixture: the thumb moved with the width, {after:?} after {wide:?}"
        );
        assert!(
            (after.center().x - thumb.center().x).abs() < 0.51,
            "centred on the thumb where it is now: {after:?} over {thumb:?}",
        );

        // A value the application writes moves it too, with no handler run.
        value.set(-6.0);
        tree.layout(narrow);
        let (text, written) = readout(&tree).expect("still up");
        let thumb = knob(&tree, s);
        assert_eq!(text, "-6.0 dB");
        assert!(
            (written.center().x - thumb.center().x).abs() < 0.51,
            "over the thumb the write moved: {written:?} over {thumb:?}",
        );
    }

    #[test]
    fn a_hover_the_slider_was_never_told_ended_does_not_hold_its_readout() {
        use teksilo_core::accesskit::Action;

        let enabled = Signal::new(true);
        let mut tree = readout_tree();
        let above = tree.add(FixedSize::new().height(10.0));
        let next = tree.add(crate::button::Button::new(lit!("Next")));
        let s = between(
            above,
            Slider::new(Signal::new(0.0_f32), -12.0, 12.0)
                .step(1.0)
                .enabled(enabled.clone())
                .value_tooltip(db),
            next,
            &mut tree,
        );
        tree.pointer_move(tree.bounds(s).center());
        tree.layout(window());
        assert!(readout(&tree).is_some());

        // Disabled under the pointer, which then leaves: a disabled slider
        // hears no `PointerLeave`.
        enabled.set(false);
        tree.layout(window());
        tree.pointer_move(away());
        enabled.set(true);
        tree.layout(window());
        tree.layout(window());
        assert_eq!(readout(&tree), None, "nothing is hovering it");

        assert!(access(&mut tree, s, Action::Increment, None));
        tree.layout(window());
        assert_eq!(
            readout(&tree),
            None,
            "an assistive step on a slider nobody is hovering raises nothing",
        );

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(s));
        tree.layout(window());
        assert!(readout(&tree).is_some(), "keyboard focus raises it");
        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(next));
        tree.layout(window());
        assert_eq!(
            readout(&tree),
            None,
            "and with focus gone and nothing hovering it, it goes",
        );
    }

    #[test]
    fn escape_takes_a_hover_readout_down_and_still_reaches_the_focused_field() {
        // A tooltip does not spend the key, and neither does the readout: the
        // user pressed Escape for the field they are typing in.
        use teksilo_core::event::EventResponse;
        use teksilo_core::widget_builder::WidgetBuilder;

        let heard = Rc::new(Cell::new(0_u32));
        let sink = heard.clone();
        let mut tree = readout_tree();
        let above = tree.add(FixedSize::new().height(10.0));
        let field = tree.add(
            FixedSize::new()
                .width(100.0)
                .height(24.0)
                .focusable(true)
                .on_key(move |event, _ctx| match event {
                    WidgetEvent::KeyDown {
                        key: Key::Escape, ..
                    } => {
                        sink.set(sink.get() + 1);
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }),
        );
        let s = between(
            above,
            Slider::new(Signal::new(0.0_f32), -12.0, 12.0).value_tooltip(db),
            field,
            &mut tree,
        );
        tree.focus(field);
        tree.pointer_move(knob(&tree, s).center());
        tree.layout(window());
        assert!(readout(&tree).is_some(), "the pointer raised it");

        tree.press_key(Key::Escape, Modifiers::NONE);
        tree.layout(window());
        assert_eq!(readout(&tree), None, "Escape took the readout down");
        assert_eq!(heard.get(), 1, "and the field heard the Escape too");
        assert_eq!(tree.focused(), Some(field), "with focus left alone");
    }

    #[test]
    fn clicks_after_the_second_in_a_burst_leave_the_default_alone() {
        let value = Signal::new(6.0_f32);
        let seen: Rc<RefCell<Vec<f32>>> = Rc::default();
        let sink = seen.clone();
        let (mut tree, s) = horizontal_scene(
            Slider::new(value.clone(), -12.0, 12.0)
                .default_value(0.0)
                .on_change(move |now, _ctx| sink.borrow_mut().push(now)),
        );
        let bounds = tree.bounds(s);
        let at = Point::new(bounds.x + bounds.width * 0.8, bounds.center().y);
        tree.pointer_move(at);
        // Five clicks in one burst. `TapStreak` stops counting at three and
        // starts again at one on the fourth, so the third and the fourth are
        // the two that used to jump.
        for click in 1..=5 {
            tree.pointer_down_button(at, PointerButton::Primary);
            tree.pointer_up_button(at, PointerButton::Primary);
            if click >= 2 {
                assert_eq!(
                    value.get(),
                    0.0,
                    "click {click} of the burst left the default where it was"
                );
            }
            tree.advance_time(std::time::Duration::from_millis(100));
        }
        let seen = seen.borrow().clone();
        assert_eq!(seen.len(), 2, "one jump and one reset: {seen:?}");
        assert_eq!(seen[1], 0.0);
    }

    #[test]
    fn focus_leaving_a_hovered_slider_keeps_its_readout_in_both_orientations() {
        for orientation in [Orientation::Horizontal, Orientation::Vertical] {
            let mut tree = readout_tree();
            let s = tree.add(
                Slider::new(Signal::new(0.0_f32), -12.0, 12.0)
                    .orientation(orientation)
                    .value_tooltip(db),
            );
            let next = tree.add(crate::button::Button::new(lit!("Next")));
            let sized = match orientation {
                Orientation::Horizontal => FixedSize::new().width(160.0).child(s),
                Orientation::Vertical => FixedSize::new().height(150.0).child(s),
            };
            let _root = tree.add(
                VStack::new().child(FixedSize::new().height(80.0)).child(
                    HStack::new()
                        .child(FixedSize::new().width(120.0))
                        .child(sized)
                        .child(next),
                ),
            );
            tree.layout(window());
            tree.press_key(Key::Tab, Modifiers::NONE);
            assert_eq!(tree.focused(), Some(s));
            tree.pointer_move(knob(&tree, s).center());
            tree.layout(window());
            assert!(readout(&tree).is_some(), "{orientation:?}: up");

            tree.press_key(Key::Tab, Modifiers::NONE);
            assert_eq!(tree.focused(), Some(next));
            tree.layout(window());
            assert!(
                readout(&tree).is_some(),
                "{orientation:?}: focus left, but the pointer is still on it",
            );
        }
    }

    #[test]
    fn another_contacts_cancel_leaves_a_live_drag_running() {
        let value = Signal::new(0.0_f32);
        let (mut tree, s) = horizontal_scene(Slider::new(value.clone(), 0.0, 100.0));
        let bounds = tree.bounds(s);
        let start = Point::new(bounds.x + 40.0, bounds.center().y);
        let dragging = tree.new_contact();
        tree.touch_down(dragging, start);
        for dx in [30.0, 60.0] {
            tree.touch_move(dragging, Point::new(start.x + dx, start.y));
        }
        let held = value.get();
        assert!(held > 0.0, "fixture: the drag is live");

        let other = tree.new_contact();
        let elsewhere = Point::new(bounds.x + 300.0, bounds.center().y);
        tree.touch_down(other, elsewhere);
        tree.touch_cancel(other, elsewhere);

        tree.touch_move(dragging, Point::new(start.x + 150.0, start.y));
        assert!(
            value.get() > held,
            "the first finger's drag carried on: {} after {held}",
            value.get()
        );
        tree.touch_up(dragging, Point::new(start.x + 150.0, start.y));
        tree.assert_no_leaked_pointer_state();
    }

    #[test]
    fn a_disabled_slider_does_not_offer_its_reset() {
        use teksilo_core::accesskit::Action;

        let enabled = Signal::new(true);
        let mut tree = WidgetTree::new();
        let s = tree.add(
            Slider::new(Signal::new(6.0_f32), -12.0, 12.0)
                .enabled(enabled.clone())
                .default_value(0.0),
        );
        let p = SizeProposal::exact(200.0, 60.0);
        let offered = |tree: &mut WidgetTree| {
            at_node(tree, s, |n| {
                (
                    n.supports_action(Action::CustomAction),
                    n.custom_actions().iter().any(|a| a.id == RESET_ACTION_ID),
                )
            })
        };
        tree.layout(p);
        assert_eq!(offered(&mut tree), (true, true));

        enabled.set(false);
        tree.layout(p);
        assert_eq!(
            offered(&mut tree),
            (false, false),
            "an action that would do nothing is not offered"
        );

        enabled.set(true);
        tree.layout(p);
        assert_eq!(
            offered(&mut tree),
            (true, true),
            "and comes back with the slider"
        );
    }
}
