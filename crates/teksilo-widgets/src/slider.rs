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

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::event::{EventResponse, PointerButton, WidgetEvent};
use teksilo_core::focus::FocusOrigin;
use teksilo_core::gesture::DragPhase;
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
            .cursor(CursorIcon::Pointer);

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
        _ctx: &LayoutContext,
    ) {
        // Cache bounds for event handling (needed before paint).
        self.cached_bounds.set(bounds);
        if let Some(child) = children.first_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.body_id.into_iter().collect()
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
            WidgetEvent::PointerDown {
                position: top,
                button: PointerButton::Primary,
                modifiers: Modifiers::NONE,
            },
            WidgetEvent::PointerUp {
                position: top,
                button: PointerButton::Primary,
                modifiers: Modifiers::NONE,
            },
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
            WidgetEvent::PointerDown {
                position: Point::new(140.0, 30.0),
                button: PointerButton::Primary,
                modifiers: Modifiers::NONE,
            },
            WidgetEvent::PointerUp {
                position: Point::new(140.0, 30.0),
                button: PointerButton::Primary,
                modifiers: Modifiers::NONE,
            },
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
}
