//! `SpinBox` — numeric input with increment/decrement buttons.
//!
//! A generic composite over [`SpinValue`](self::value::SpinValue)
//! (integer and floating-point primitives), pairing the
//! [`TextInputField`](crate::primitives::TextInputField) editing
//! primitive with a stacked pair of up/down step buttons. Semantics
//! are a synthesis of Qt's `QSpinBox` / `QDoubleSpinBox`, WinUI 3's
//! `NumberBox`, GTK's `GtkSpinButton`, and the W3C ARIA
//! `spinbutton` role.
//!
//! # Behaviour
//!
//! - **Value binding**: a `Signal<T>` is the single source of truth.
//!   Typing and stepping update it; external writes re-format the
//!   editable text.
//! - **Commit model**: the user can type freely (subject to the
//!   per-character input filter). The value is *committed* on
//!   [`Enter`](fern_core::event::Key::Enter) or on focus loss —
//!   at commit time the text is parsed, clamped into `[min, max]`
//!   (or wrapped, per [`WrapMode`]), and reformatted. Invalid input
//!   reverts to the last known good value.
//! - **Keyboard**:
//!   - `Up` / `Down` → ±[`single_step`](SpinBox::single_step)
//!   - `PageUp` / `PageDown` → ±[`page_step`](SpinBox::page_step)
//!     (default: `10 × single_step`)
//!   - `Enter` → commit (stays focused)
//!   - `Home` / `End` stay bound to the text cursor (Qt-compatible).
//! - **Mouse wheel**: adjusts by `single_step`; gated by
//!   [`wheel_mode`](SpinBox::wheel_mode) (default: only when
//!   focused, to avoid accidental scroll changes).
//! - **Buttons**: up/down buttons stack to the right of the field
//!   by default; can be hidden with
//!   [`button_layout`](SpinBox::button_layout).
//! - **Special value text**: when the current value equals `min`
//!   and [`special_value_text`](SpinBox::special_value_text) is
//!   set, the field shows that string instead of the formatted
//!   number — Qt's "Auto" / "None" / "Unlimited" affordance.
//! - **Adaptive step**: with
//!   [`StepType::Adaptive`](StepType::Adaptive), the effective step
//!   tracks the decimal magnitude of the current value (Qt's
//!   `AdaptiveDecimalStepType`). Useful for values that span many
//!   orders of magnitude in the same control.
//! - **Custom formatter / parser**: full override via
//!   [`text_from_value`](SpinBox::text_from_value) and
//!   [`value_from_text`](SpinBox::value_from_text); together they
//!   let you implement currency, percentages with stored fraction,
//!   hex, duration, anything.
//!
//! # Accessibility
//!
//! The composite exposes itself as
//! [`Role::SpinButton`](fern_core::accesskit::Role::SpinButton)
//! with numeric value, min, max, step, and jump properties set on
//! the AccessKit node; the AT receives
//! [`Increment`](fern_core::accesskit::Action::Increment),
//! [`Decrement`](fern_core::accesskit::Action::Decrement),
//! [`SetValue`](fern_core::accesskit::Action::SetValue), and
//! [`Focus`](fern_core::accesskit::Action::Focus) actions. The
//! step buttons are structurally part of the SpinBox and publish
//! no separate a11y nodes.
//!
//! # Example
//!
//! ```ignore
//! use fern_ui::widgets::{SpinBox, WrapMode};
//!
//! let font_size = ctx.signal(12_i32);
//! ctx.add(
//!     SpinBox::new(font_size, 4, 72)
//!         .single_step(1)
//!         .page_step(10)
//!         .suffix(" pt"),
//! );
//!
//! let gain_db = ctx.signal(0.0_f32);
//! ctx.add(
//!     SpinBox::new(gain_db, -60.0, 12.0)
//!         .single_step(0.5)
//!         .decimals(1)
//!         .suffix(" dB")
//!         .wrap_mode(WrapMode::Clamp),
//! );
//! ```

mod step_button;
#[cfg(test)]
mod tests;
mod value;

use std::rc::Rc;

pub use self::value::SpinValue;

use fern_canvas::{Path, Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, ScrollDelta, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::widget::{EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_text::SharedTypesetter;
use fern_tokens::{CornerRadius, TextStyle};

use crate::button::InteractionState;
use crate::primitives::icon_widget::IconWidget;
use crate::primitives::text_input_field::TextInputField;
use crate::primitives::{
    Divider, Expand, HStack, MinSize, Padding, RectWidget, VStack, ZStack,
};

use self::step_button::StepButton;

// ── Enums ──────────────────────────────────────────────────────────

/// Out-of-range behavior when stepping past `min` or `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WrapMode {
    /// Clamp to `min` / `max` (default).
    #[default]
    Clamp,
    /// Wrap around: past `max` jumps to `min`, past `min` jumps to
    /// `max`. Matches Qt's `QAbstractSpinBox::wrapping`.
    Wrap,
}

/// Step-size policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StepType {
    /// Always step by `single_step` (default).
    #[default]
    Fixed,
    /// Step by the decimal power-of-ten immediately below the
    /// current value's magnitude — e.g. values 1–9 step by 1,
    /// 10–99 by 10, 100–999 by 100. Matches Qt's
    /// `AdaptiveDecimalStepType`. Integer types honor the same
    /// rule using the magnitude of the absolute value.
    Adaptive,
}

/// Button visibility / placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonLayout {
    /// Up arrow on top, down arrow below, stacked vertically to the
    /// right of the field. Default, matches Qt and WinUI.
    #[default]
    Stacked,
    /// No visible step buttons. Useful for read-only displays and
    /// for SpinBoxes driven entirely by keyboard / wheel.
    Hidden,
}

/// When the mouse wheel adjusts the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WheelMode {
    /// Wheel adjusts only when the field is focused. Default —
    /// prevents accidental changes when the user is scrolling a
    /// larger surrounding view.
    #[default]
    Focused,
    /// Wheel adjusts whenever the pointer is over the widget.
    Hover,
    /// Wheel never adjusts the value; events bubble to the
    /// surrounding scroll container.
    Disabled,
}

/// How the SpinBox decides its horizontal size envelope.
///
/// Chosen via the [`width`](SpinBox::width),
/// [`width_chars`](SpinBox::width_chars), and
/// [`fill_width`](SpinBox::fill_width) builder methods — the enum
/// itself is the storage, not a separate public configuration
/// API.
#[derive(Debug, Clone)]
pub enum WidthPolicy {
    /// Cap the widget at a fixed logical-pixel width. Default is
    /// [`DEFAULT_PREFERRED_WIDTH`] (120 dp), matching Qt's
    /// `QSpinBox` sizeHint.
    Pixels(f32),
    /// Size the widget to fit this many reference digits (`'0'`)
    /// plus the configured suffix, padding, and step buttons.
    /// Measurement uses the theme font at build time.
    Chars(u32),
    /// Let the widget expand horizontally to fill whatever space
    /// the parent offers. Equivalent to an infinite pixel cap.
    Fill,
}

// ── Type aliases for builder closures ──────────────────────────────

type TextFromValue<T> = Rc<dyn Fn(T) -> String>;
type ValueFromText<T> = Rc<dyn Fn(&str) -> Option<T>>;
type OnValueChangedFn<T> = Rc<dyn Fn(T, &mut EventContext)>;

/// Minimum total width. Below this the stacked step buttons stop
/// fitting next to the field. Widgets narrower than this are
/// enforced to the minimum at layout time via `MinSize`.
const MIN_WIDTH_WITH_BUTTONS: f32 = 72.0;
/// Minimum total width when buttons are hidden — the field alone
/// plus padding still reads as a numeric control.
const MIN_WIDTH_NO_BUTTONS: f32 = 48.0;
/// Default maximum width. Matches Qt's `QSpinBox` sizeHint for a
/// 4-digit value + unit suffix and stays tight in Int UI-style
/// dense forms.
const DEFAULT_PREFERRED_WIDTH: f32 = 120.0;

// ── SpinBox ────────────────────────────────────────────────────────

/// Numeric input with step buttons. Generic over
/// [`SpinValue`] — pre-implemented for `i32`, `i64`, `u32`, `u64`,
/// `usize`, `f32`, and `f64`.
pub struct SpinBox<T: SpinValue> {
    // ── Required configuration ──────────────────────────────────────
    value: Signal<T>,
    min: T,
    max: T,

    // ── Optional configuration (builders) ───────────────────────────
    single_step: T,
    page_step: Option<T>,
    decimals: u8,
    suffix: String,
    special_value_text: Option<String>,
    wrap_mode: WrapMode,
    step_type: StepType,
    button_layout: ButtonLayout,
    wheel_mode: WheelMode,
    /// Horizontal sizing policy. One of [`WidthPolicy::Pixels`]
    /// (fixed cap), [`WidthPolicy::Chars`] (font-metric-based),
    /// or [`WidthPolicy::Fill`] (stretch to parent). Set by the
    /// [`width`](SpinBox::width), [`width_chars`](SpinBox::width_chars),
    /// and [`fill_width`](SpinBox::fill_width) builder methods.
    width_policy: WidthPolicy,
    label: Option<String>,
    placeholder: String,
    enabled: bool,
    read_only: bool,
    text_from_value: Option<TextFromValue<T>>,
    value_from_text: Option<ValueFromText<T>>,
    on_value_changed: Option<OnValueChangedFn<T>>,

    // ── Internal state (set during build) ───────────────────────────
    text_signal: Signal<String>,
    interaction: Signal<InteractionState>,
    can_step_up: Signal<bool>,
    can_step_down: Signal<bool>,
    /// Cached horizontal cap in pixels, resolved from `width_policy`
    /// at build time (Chars mode measures the theme font). `None`
    /// when the policy is `Fill`. Applied by `size_that_fits` by
    /// narrowing the proposal before delegating to the child — this
    /// replaces wrapping the subtree in a `MaxSize`, which clips
    /// children and would truncate the focus-state border stroke
    /// against its own shape quad.
    pixel_cap: Option<f32>,
    /// Floor width so the field and step buttons always fit. Also
    /// resolved at build from `button_layout`.
    min_width: f32,
    root_child_id: Option<WidgetId>,
    field_id: Option<WidgetId>,
}

impl<T: SpinValue> std::fmt::Debug for SpinBox<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpinBox")
            .field("min", &self.min)
            .field("max", &self.max)
            .field("single_step", &self.single_step)
            .field("decimals", &self.decimals)
            .field("wrap_mode", &self.wrap_mode)
            .finish_non_exhaustive()
    }
}

impl<T: SpinValue> SpinBox<T> {
    /// Construct a new SpinBox bound to `value` with the given
    /// inclusive range. `min` must be ≤ `max`.
    pub fn new(value: Signal<T>, min: T, max: T) -> Self {
        // `single_step` defaults to 1 on the value's f64 scale —
        // works naturally for integers and for decimal floats.
        // Callers with a different natural step (0.1, 0.5, 10, …)
        // override via `single_step(...)`.
        let default_step = T::from_f64_saturating(1.0);
        Self {
            value,
            min,
            max,
            single_step: default_step,
            page_step: None,
            decimals: if T::is_integer() { 0 } else { 2 },
            suffix: String::new(),
            special_value_text: None,
            wrap_mode: WrapMode::Clamp,
            step_type: StepType::Fixed,
            button_layout: ButtonLayout::Stacked,
            wheel_mode: WheelMode::Focused,
            width_policy: WidthPolicy::Pixels(DEFAULT_PREFERRED_WIDTH),
            label: None,
            placeholder: String::new(),
            enabled: true,
            read_only: false,
            text_from_value: None,
            value_from_text: None,
            on_value_changed: None,
            text_signal: Signal::new(String::new()),
            interaction: Signal::new(InteractionState::Idle),
            can_step_up: Signal::new(true),
            can_step_down: Signal::new(true),
            pixel_cap: None,
            min_width: MIN_WIDTH_WITH_BUTTONS,
            root_child_id: None,
            field_id: None,
        }
    }

    // ── Builder methods ─────────────────────────────────────────────

    /// Set the step size for `Up` / `Down` / single wheel tick /
    /// button tap.
    pub fn single_step(mut self, step: T) -> Self {
        self.single_step = step;
        self
    }

    /// Set the step size for `PageUp` / `PageDown`. When unset,
    /// defaults to `10 × single_step` at build time.
    pub fn page_step(mut self, step: T) -> Self {
        self.page_step = Some(step);
        self
    }

    /// Number of decimal places shown for floating-point types.
    /// Ignored for integer types.
    pub fn decimals(mut self, decimals: u8) -> Self {
        self.decimals = decimals;
        self
    }

    /// Qt-style non-editable trailing unit (e.g. `" %"`, `" px"`,
    /// `" dB"`). Rendered flush-right inside the field's border;
    /// the caret cannot enter it.
    pub fn suffix(mut self, text: impl Into<String>) -> Self {
        self.suffix = text.into();
        self
    }

    /// Text shown in place of the formatted value when the current
    /// value equals `min`. Use for "Auto", "None", "Off",
    /// "Unlimited" affordances where the minimum has special
    /// semantics. When the field is focused the real number is
    /// shown instead so the user can type.
    pub fn special_value_text(mut self, text: impl Into<String>) -> Self {
        self.special_value_text = Some(text.into());
        self
    }

    pub fn wrap_mode(mut self, mode: WrapMode) -> Self {
        self.wrap_mode = mode;
        self
    }

    pub fn step_type(mut self, step_type: StepType) -> Self {
        self.step_type = step_type;
        self
    }

    pub fn button_layout(mut self, layout: ButtonLayout) -> Self {
        self.button_layout = layout;
        self
    }

    /// Convenience wrapper over [`button_layout`](Self::button_layout):
    /// `true` → `ButtonLayout::Stacked`, `false` → `ButtonLayout::Hidden`.
    /// Matches the Int UI guideline that SpinBoxes in dense forms
    /// often hide the step buttons to reduce visual noise and let
    /// keyboard / wheel carry the affordance — pass
    /// `.show_buttons(false)` on those call sites.
    pub fn show_buttons(mut self, show: bool) -> Self {
        self.button_layout = if show {
            ButtonLayout::Stacked
        } else {
            ButtonLayout::Hidden
        };
        self
    }

    pub fn wheel_mode(mut self, mode: WheelMode) -> Self {
        self.wheel_mode = mode;
        self
    }

    /// Cap the widget's horizontal size at a fixed logical-pixel
    /// width. If the parent offers less, the SpinBox shrinks (down
    /// to the internal 72 dp / 48 dp floor that keeps the buttons
    /// and field from overlapping). Default: 120 dp, matching Qt
    /// `QSpinBox` sizeHint and Int UI form density.
    ///
    /// ```ignore
    /// SpinBox::new(v, 0, 9999).width(80.0)        // narrow
    /// SpinBox::new(v, 0, 9999).width(200.0)       // wider
    /// SpinBox::new(v, 0, 9999).fill_width()       // stretch to parent
    /// SpinBox::new(v, 0, 9999).width_chars(5)     // "fits 5 digits"
    /// ```
    pub fn width(mut self, width: f32) -> Self {
        self.width_policy = WidthPolicy::Pixels(width.max(0.0));
        self
    }

    /// Size the widget to fit exactly `chars` reference digits plus
    /// the configured suffix, padding, and step buttons. The
    /// measurement uses the actual theme font at build time (same
    /// `SharedTypesetter` the field draws with), so values stay
    /// right under runtime theme switches and HiDPI scale changes.
    ///
    /// ```ignore
    /// SpinBox::new(port, 0, 65_535).width_chars(5)          // 5 digits
    /// SpinBox::new(pct, 0, 100).suffix(" %").width_chars(3) // 3 + " %"
    /// ```
    pub fn width_chars(mut self, chars: u32) -> Self {
        self.width_policy = WidthPolicy::Chars(chars);
        self
    }

    /// Let the widget expand to fill the horizontal space offered
    /// by its parent, instead of capping at [`width`](Self::width).
    /// Use inside toolbars, inspector panels, or an
    /// `Expand::horizontal` column that should stretch with the
    /// surrounding layout.
    pub fn fill_width(mut self) -> Self {
        self.width_policy = WidthPolicy::Fill;
        self
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Override the value → display-string conversion. Receives the
    /// raw value; returns whatever string should appear in the
    /// field. Suffix and `special_value_text` still apply on top of
    /// the returned string.
    pub fn text_from_value(mut self, f: impl Fn(T) -> String + 'static) -> Self {
        self.text_from_value = Some(Rc::new(f));
        self
    }

    /// Override the parse step. Receives the field's raw text
    /// (without the suffix, which is never part of the editable
    /// content); returns `Some(value)` to accept or `None` to
    /// reject. Invalid input reverts to the last good value on
    /// commit.
    pub fn value_from_text(mut self, f: impl Fn(&str) -> Option<T> + 'static) -> Self {
        self.value_from_text = Some(Rc::new(f));
        self
    }

    /// Closure fired each time the value is committed (keyboard
    /// step, button tap, wheel tick, Enter, blur). Bound observers
    /// on the value signal also see every change; use this hook
    /// when the caller needs an `EventContext` (e.g. to fire an
    /// intent).
    pub fn on_value_changed(mut self, f: impl Fn(T, &mut EventContext) + 'static) -> Self {
        self.on_value_changed = Some(Rc::new(f));
        self
    }

    // ── Signal accessors (call before add to tree) ──────────────────

    /// The bound numeric value signal.
    pub fn value(&self) -> Signal<T> {
        self.value.clone()
    }
}

impl<T: SpinValue> Widget for SpinBox<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Sanity-check the range once. A malformed range is a
        // programming error, not a runtime user error.
        debug_assert!(
            self.min <= self.max,
            "SpinBox min must be <= max"
        );

        // SpinBox reads theme tokens once for static layout params
        // (padding, focus-ring width). Color-bearing leaf widgets below
        // use `theme_signal.map(...)` so runtime theme switches still
        // re-paint. Full reactivity of layout params would require
        // migrating every `Prop<f32>`-accepting primitive used here.
        let theme_signal = ctx.theme_signal();
        let theme = theme_signal.get();
        let colors = theme.colors.clone();
        let field_style = theme.components.text_field;
        let focus_ring_width = theme.shape.focus_ring_width;

        // Capture configuration into owned clones for the effect
        // closures. The builder closures are `Rc`-wrapped already.
        let min = self.min;
        let max = self.max;
        let decimals = self.decimals;
        let suffix_str = self.suffix.clone();
        let special_text = self.special_value_text.clone();
        let text_from_value = self.text_from_value.clone();
        let value_from_text = self.value_from_text.clone();
        let wrap_mode = self.wrap_mode;
        let step_type = self.step_type;
        let single_step = self.single_step;
        let page_step = self.page_step.unwrap_or_else(|| {
            single_step.saturating_mul_u32(10)
        });
        let on_value_changed = self.on_value_changed.clone();
        let enabled = self.enabled;
        let read_only = self.read_only;
        let wheel_mode = self.wheel_mode;

        // Seed the text signal from the current value.
        {
            let initial = format_for_display(
                self.value.get(),
                decimals,
                special_text.as_deref(),
                text_from_value.as_deref(),
                min,
                false,
            );
            self.text_signal.set(initial);
        }

        // Effect: when the value signal changes externally, reformat
        // the text. This also fires on startup with the initial value
        // (guaranteed by `ctx.effect`). Skipped when the field is
        // focused so typing isn't interrupted by our own round-trip
        // writes — on commit we explicitly re-sync.
        {
            let text_signal = self.text_signal.clone();
            let text_from_value = text_from_value.clone();
            let special_text = special_text.clone();
            let interaction = self.interaction.clone();
            let can_up = self.can_step_up.clone();
            let can_down = self.can_step_down.clone();
            let min_cap = min;
            let max_cap = max;
            ctx.effect(&self.value, move |new_value| {
                // Update the can-step signals any time the value
                // changes so the buttons and a11y reflect whether
                // further stepping is possible under clamp mode.
                let is_focused = interaction.get() == InteractionState::Focused;
                if wrap_mode == WrapMode::Wrap {
                    can_up.set(true);
                    can_down.set(true);
                } else {
                    can_up.set(*new_value < max_cap);
                    can_down.set(*new_value > min_cap);
                }
                if !is_focused {
                    let formatted = format_for_display(
                        *new_value,
                        decimals,
                        special_text.as_deref(),
                        text_from_value.as_deref(),
                        min_cap,
                        false,
                    );
                    if text_signal.get() != formatted {
                        text_signal.set(formatted);
                    }
                }
            });
        }

        // Commit helper: called on Enter and on blur. Parses the
        // current text; on success, clamps and writes the value and
        // reformats the text. On failure, reverts the text to the
        // formatted current value.
        let commit: Rc<dyn Fn(&mut EventContext)> = {
            let value_signal = self.value.clone();
            let text_signal = self.text_signal.clone();
            let value_from_text = value_from_text.clone();
            let text_from_value = text_from_value.clone();
            let special_text = special_text.clone();
            let on_value_changed = on_value_changed.clone();
            Rc::new(move |ctx: &mut EventContext| {
                let raw = text_signal.get();
                let parsed: Option<T> = match value_from_text.as_deref() {
                    Some(f) => f(raw.trim()),
                    None => T::parse(raw.trim()),
                };
                let old = value_signal.get();
                let new_value = match parsed {
                    Some(v) => v.clamp_value(min, max),
                    None => old, // revert
                };
                let formatted = format_for_display(
                    new_value,
                    decimals,
                    special_text.as_deref(),
                    text_from_value.as_deref(),
                    min,
                    false,
                );
                if text_signal.get() != formatted {
                    text_signal.set(formatted);
                }
                if approx_ne(new_value, old) {
                    value_signal.set(new_value);
                    if let Some(cb) = on_value_changed.as_ref() {
                        cb(new_value, ctx);
                    }
                }
            })
        };

        // ── Step helpers ───────────────────────────────────────────
        //
        // Two closures cover the two firing pathways:
        //
        // - `step` is called from event handlers (keyboard, wheel,
        //   button tap, a11y action) and takes an `EventContext`
        //   so it can fire the user's `on_value_changed` callback
        //   and request a frame.
        //
        // - `step_silent` is called from signal-only contexts
        //   (hold-to-repeat on the step buttons, which lives in a
        //   frame-tick effect that has no `EventContext` to hand).
        //   It mutates `value` and `text_signal` and lets the
        //   bindings on those signals trigger the redraw. The
        //   user's `on_value_changed` callback is deliberately
        //   skipped — signal observers still see every change,
        //   which is the primary notification channel.
        //
        // `can_step_up` / `can_step_down` are kept in sync by the
        // value-effect above.

        fn apply_step<T: SpinValue>(
            dir: i32,
            page: bool,
            step_type: StepType,
            wrap_mode: WrapMode,
            single_step: T,
            page_step: T,
            min: T,
            max: T,
            current: T,
        ) -> T {
            let base_step = if page { page_step } else { single_step };
            let effective = resolve_effective_step(step_type, current, base_step);
            let stepped = if dir > 0 {
                current.saturating_add(effective)
            } else {
                current.saturating_sub(effective)
            };
            if stepped < min || stepped > max {
                match wrap_mode {
                    WrapMode::Clamp => stepped.clamp_value(min, max),
                    WrapMode::Wrap => {
                        if stepped > max { min } else { max }
                    }
                }
            } else {
                stepped
            }
        }

        // Signal-only step: mutates `value` and `text_signal` and
        // returns the previous/new pair so the caller can fire any
        // extra side-effect (e.g. `on_value_changed`) after the
        // fact. When nothing changed returns `None`.
        let step_silent: Rc<dyn Fn(i32, bool) -> Option<T>> = {
            let value_signal = self.value.clone();
            let text_signal = self.text_signal.clone();
            let text_from_value = text_from_value.clone();
            let special_text = special_text.clone();
            Rc::new(move |dir: i32, page: bool| {
                if read_only {
                    return None;
                }
                let current = value_signal.get();
                let new_value = apply_step(
                    dir, page, step_type, wrap_mode,
                    single_step, page_step, min, max, current,
                );
                if approx_eq(new_value, current) {
                    return None;
                }
                value_signal.set(new_value);
                let formatted = format_for_display(
                    new_value,
                    decimals,
                    special_text.as_deref(),
                    text_from_value.as_deref(),
                    min,
                    false,
                );
                if text_signal.get() != formatted {
                    text_signal.set(formatted);
                }
                Some(new_value)
            })
        };

        // Contextful step: wraps `step_silent` and fires the user
        // callback + frame request on change.
        let step: Rc<dyn Fn(i32, bool, &mut EventContext)> = {
            let step_silent = step_silent.clone();
            let on_value_changed = on_value_changed.clone();
            Rc::new(move |dir: i32, page: bool, ctx: &mut EventContext| {
                if let Some(new_value) = step_silent(dir, page) {
                    if let Some(cb) = on_value_changed.as_ref() {
                        cb(new_value, ctx);
                    }
                    ctx.request_frame();
                }
            })
        };

        // ── Inner editing field ────────────────────────────────────
        //
        // Uses `TextInputField` directly rather than the `TextInput`
        // composite — the composite's border / padding / placeholder
        // overlay is reproduced here around both the field and the
        // buttons in one shared frame, instead of framing the text
        // by itself.
        let inner_height = (field_style.height - 2.0 * field_style.border_width).max(0.0);
        let text_area_height = (inner_height - 2.0 * field_style.padding_vertical).max(0.0);

        let mut field = TextInputField::new(self.text_signal.clone())
            .enabled(enabled)
            .read_only(read_only)
            .placeholder(self.placeholder.clone())
            .text_height(text_area_height)
            .interaction_signal(self.interaction.clone())
            .char_filter(T::is_valid_input_char);
        // Suffix wiring:
        //   • plain static suffix              → `.suffix(..)` (no signal)
        //   • static suffix + special_value    → reactive: hide suffix when
        //     `value == min` AND the field isn't focused, so `"Auto"` reads
        //     cleanly without a trailing unit but typing at `min` still
        //     shows the unit. Matches Qt's `QSpinBox::specialValueText`
        //     behavior.
        //   • no suffix and no special         → nothing to do.
        //
        // The reactive case uses a mutable intermediate signal
        // rather than feeding `TextInputField::bind_suffix` a
        // derived signal directly — `ctx.effect` requires a
        // mutable source, and the field needs to drive a
        // relayout + re-measure when the suffix flips on/off.
        if !suffix_str.is_empty() {
            if self.special_value_text.is_some() {
                let suffix_live = ctx.signal(suffix_str.clone());
                let resolve = {
                    let suffix_str = suffix_str.clone();
                    let min_cap = min;
                    move |v: T, focused: bool| -> String {
                        let at_min = approx_eq(v, min_cap);
                        if at_min && !focused {
                            String::new()
                        } else {
                            suffix_str.clone()
                        }
                    }
                };
                // Seed from the current state.
                {
                    let current_focused =
                        self.interaction.get() == InteractionState::Focused;
                    suffix_live.set(resolve(self.value.get(), current_focused));
                }
                // Observe value.
                {
                    let suffix_live = suffix_live.clone();
                    let interaction = self.interaction.clone();
                    let resolve = resolve.clone();
                    ctx.effect(&self.value, move |v| {
                        let focused =
                            interaction.get() == InteractionState::Focused;
                        let next = resolve(*v, focused);
                        if suffix_live.get() != next {
                            suffix_live.set(next);
                        }
                    });
                }
                // Observe interaction.
                {
                    let suffix_live = suffix_live.clone();
                    let value_signal = self.value.clone();
                    let resolve = resolve.clone();
                    ctx.effect(&self.interaction, move |state| {
                        let focused = *state == InteractionState::Focused;
                        let next = resolve(value_signal.get(), focused);
                        if suffix_live.get() != next {
                            suffix_live.set(next);
                        }
                    });
                }
                field = field.bind_suffix(suffix_live);
            } else {
                field = field.suffix(suffix_str.clone());
            }
        }
        // Submit on Enter: commit in-place, keep focus.
        {
            let commit = commit.clone();
            field = field.on_submit_fn(move |ctx| commit(ctx));
        }
        // Commit on blur: after the field clears its selection and
        // resets scroll.
        {
            let commit = commit.clone();
            field = field.on_blur_fn(move |ctx| commit(ctx));
        }

        // Also: when the field gains focus, show the raw editable
        // text instead of any `special_value_text`. TextInputField
        // itself doesn't know about our formatter so we bind a
        // secondary effect on its interaction signal.
        {
            let interaction_for_focus_text = self.interaction.clone();
            let text_signal = self.text_signal.clone();
            let value_signal = self.value.clone();
            let text_from_value = text_from_value.clone();
            let min_cap = min;
            ctx.effect(&interaction_for_focus_text, move |state| {
                if *state == InteractionState::Focused {
                    // On focus, swap any special_value_text out for
                    // the plain formatted number so the user can
                    // edit it with the keyboard.
                    let plain = format_for_display(
                        value_signal.get(),
                        decimals,
                        None,
                        text_from_value.as_deref(),
                        min_cap,
                        true,
                    );
                    if text_signal.get() != plain {
                        text_signal.set(plain);
                    }
                }
            });
        }

        let field_id = ctx.add(field);
        self.field_id = Some(field_id);

        // Wrap field in vertical padding so it aligns inside the frame.
        let padded_field_id = ctx.add(
            Padding::new(
                field_style.padding_vertical, 0.0,
                field_style.padding_vertical, 0.0,
            )
            .child_id(field_id),
        );
        // `fills_stack()` is critical here: without it the HStack
        // below treats Expand as a natural-sized child and queries
        // `TextInputField::size_that_fits` with `width = None`, which
        // falls back to the primitive's 200 dp default. That natural
        // width then adds up with the divider + buttons, blows past
        // the MaxSize cap, and the field paints off-screen. With
        // `fills_stack`, Expand reports as a spacer and the HStack
        // assigns it exactly the leftover width inside the
        // constrained bounds.
        let expanded_field_id = ctx.add(
            Expand::horizontal().fills_stack().child_id(padded_field_id),
        );

        // ── Step button column ─────────────────────────────────────
        let buttons_id_opt = if self.button_layout != ButtonLayout::Hidden {
            Some(build_buttons(
                ctx,
                &step,
                &step_silent,
                self.can_step_up.clone(),
                self.can_step_down.clone(),
                enabled && !read_only,
                field_style.height,
                field_style.corner_radius,
            ))
        } else {
            None
        };

        // ── Row: field | divider | buttons ────────────────────────
        let row_id = {
            let mut row = HStack::new().spacing(0.0);
            row = row.add_child(expanded_field_id);
            if let Some(buttons_id) = buttons_id_opt {
                // Thin vertical divider between text and buttons so
                // the click targets read as distinct affordances.
                let divider = Divider::vertical()
                    .thickness(1.0)
                    .color(fern_tokens::BorderRole::Default);
                let divider_id = ctx.add(
                    Padding::new(2.0, 0.0, 2.0, 0.0).child(divider),
                );
                row = row.add_child(divider_id).add_child(buttons_id);
            }
            ctx.add(row)
        };

        // Symmetric horizontal padding sourced from
        // `theme.components.text_field.padding_horizontal` — same
        // token `TextInput` uses, so the two controls line up on
        // forms. Previously the left side was set to 0, which made
        // the text sit flush against the border; the chrome math
        // for `.width_chars()` already assumes padding on both
        // sides (`padding_horizontal * 2.0`), so this also brings
        // the measured and rendered widths back into agreement.
        let padded_row_id = ctx.add(
            Padding::new(
                0.0,
                field_style.padding_horizontal,
                0.0,
                field_style.padding_horizontal,
            )
            .child_id(row_id),
        );

        // ── Frame: border + background ─────────────────────────────
        //
        // Int UI text-field convention (Section 7 of the v2
        // reference): the focus indicator IS the border — accent-
        // colored and `focus_ring_width` dp thick when focused,
        // normal border color and `border_width` dp otherwise. No
        // separate ring wrapping the control (which would reserve
        // extra envelope space and clash with row layouts that
        // expect the widget to report its visual footprint as its
        // full size).
        // Border color tracks both interaction AND theme — zip combines
        // both roots so focus transitions and runtime theme switches both
        // refresh the rendered border.
        let border_color = self
            .interaction
            .zip(&theme_signal)
            .map(|(state, t)| match *state {
                InteractionState::Focused => t.colors.focus_ring,
                _ => t.colors.border,
            });
        let border_width_signal =
            self.interaction
                .zip(&theme_signal)
                .map(|(state, t)| match *state {
                    InteractionState::Focused => t.shape.focus_ring_width,
                    _ => t.components.text_field.border_width,
                });
        let _ = focus_ring_width; // retained for the reactive closure above
        let bg = RectWidget::new()
            .background(fern_tokens::SurfaceRole::Content)
            .border_color(border_color)
            .border_width(border_width_signal)
            .corner_radius(CornerRadius::uniform(field_style.corner_radius));
        let bg_id = ctx.add(bg);

        let zstack_id = ctx.add(
            ZStack::new().add_child(bg_id).add_child(padded_row_id),
        );

        // Resolve the width policy into a concrete pixel cap (or
        // `None` for `Fill`). Char-mode measurement uses the app-
        // wide `SharedTypesetter` — same backend the field paints
        // with — so the result tracks runtime theme switches and
        // HiDPI scale changes. `'0'` is the reference digit since
        // Inter and most UI sans-serifs ship tabular-figure
        // numerals; the suffix is measured separately because it
        // may have different glyph advances (e.g. `" %"`).
        let min_width = match self.button_layout {
            ButtonLayout::Stacked => MIN_WIDTH_WITH_BUTTONS,
            ButtonLayout::Hidden => MIN_WIDTH_NO_BUTTONS,
        };
        let pixel_cap: Option<f32> = match self.width_policy {
            WidthPolicy::Fill => None,
            WidthPolicy::Pixels(px) => Some(px.max(min_width)),
            WidthPolicy::Chars(chars) => {
                let style = &theme.typography.body;
                let sample: String = "0".repeat(chars as usize);
                let digits_w = measure_width_px(ctx, &sample, style);
                let suffix_w = if suffix_str.is_empty() {
                    0.0
                } else {
                    measure_width_px(ctx, &suffix_str, style)
                };
                let button_chrome = match self.button_layout {
                    // 18 dp button + 4 dp divider padding + 1 dp divider
                    ButtonLayout::Stacked => 18.0 + 4.0 + 1.0,
                    ButtonLayout::Hidden => 0.0,
                };
                // 2 dp slack so the caret and a trailing zero never
                // paint flush against the right edge.
                let chrome = field_style.padding_horizontal * 2.0 + button_chrome + 2.0;
                Some((digits_w + suffix_w + chrome).max(min_width))
            }
        };

        // Size envelope:
        //   MinSize  → enforce a floor so the field and buttons
        //              still fit even when a narrow parent would
        //              otherwise squash the widget.
        //   The horizontal cap (when `pixel_cap` is `Some`) is
        //   applied by `SpinBox::size_that_fits` narrowing the
        //   proposal, NOT by wrapping in `MaxSize`. `MaxSize`
        //   clips its children, which would truncate the outer
        //   half of the focus-state border stroke against the
        //   widget's own shape quad (visible as a ring clipped on
        //   all four sides).
        let sized_id = ctx.add(
            MinSize::new(min_width, field_style.height).child_id(zstack_id),
        );
        // Stash the resolved cap + floor on `self` for
        // `size_that_fits` to read at layout time.
        self.pixel_cap = pixel_cap;
        self.min_width = min_width;

        // ── Root: attach key + wheel handlers on the outer sized id ─
        //
        // Bubble-phase `on_key` catches Up / Down / PageUp / PageDown
        // after the `TextInputField` declines them (the field's
        // keyboard dispatch falls through to `_ =>` for arrow keys,
        // returning `Ignored` so the bubble loop continues up).
        let root_id = sized_id;
        self.root_child_id = Some(root_id);

        let step_for_key = step.clone();
        let step_for_wheel = step.clone();
        let value_for_a11y = self.value.clone();
        let field_id_for_access = field_id;

        let handlers = HandlerSet::new()
            .on_key(move |event, ctx| {
                if !enabled || read_only {
                    return EventResponse::Ignored;
                }
                let WidgetEvent::KeyDown { key, .. } = event else {
                    return EventResponse::Ignored;
                };
                match key {
                    Key::ArrowUp => { (step_for_key)(1, false, ctx); EventResponse::Handled }
                    Key::ArrowDown => { (step_for_key)(-1, false, ctx); EventResponse::Handled }
                    Key::PageUp => { (step_for_key)(1, true, ctx); EventResponse::Handled }
                    Key::PageDown => { (step_for_key)(-1, true, ctx); EventResponse::Handled }
                    _ => EventResponse::Ignored,
                }
            })
            .on_scroll({
                let interaction = self.interaction.clone();
                move |event, ctx| {
                    if !enabled || read_only || wheel_mode == WheelMode::Disabled {
                        return EventResponse::Ignored;
                    }
                    // `Focused` wheel mode only fires when the
                    // inner field currently holds focus. `Hover` is
                    // the natural fallthrough — scroll events reach
                    // the widget only when the pointer is over it.
                    if wheel_mode == WheelMode::Focused
                        && interaction.get() != InteractionState::Focused
                    {
                        return EventResponse::Ignored;
                    }
                    let WidgetEvent::Scroll { delta, .. } = event else {
                        return EventResponse::Ignored;
                    };
                    let y = match delta {
                        ScrollDelta::Lines { y, .. } => *y,
                        ScrollDelta::Pixels { y, .. } => *y,
                    };
                    if y == 0.0 {
                        return EventResponse::Ignored;
                    }
                    // Positive delta_y means scrolling up on most
                    // systems; step up by one unit per tick.
                    let dir = if y > 0.0 { 1 } else { -1 };
                    (step_for_wheel)(dir, false, ctx);
                    EventResponse::Handled
                }
            })
            .on_access_action(move |action, ctx| {
                use fern_core::accesskit::Action;
                match action {
                    Action::Increment => {
                        (step.clone())(1, false, ctx);
                        EventResponse::Handled
                    }
                    Action::Decrement => {
                        (step.clone())(-1, false, ctx);
                        EventResponse::Handled
                    }
                    Action::Focus => {
                        ctx.request_focus(field_id_for_access);
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            });
        // Bind `value` so the SpinButton a11y node refreshes on
        // every change (numeric_value setter reads it live).
        let self_id = ctx.self_id();
        value_for_a11y.bind_to(
            self_id,
            ctx.binding_registry(),
            fern_core::binding::BindingLevel::AccessibilityOnly,
        );

        ctx.apply_self_handlers(handlers);

        if !enabled {
            self.interaction.set(InteractionState::Disabled);
        }

        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        // Narrow the parent's proposal by `pixel_cap` (if any)
        // before delegating. This enforces the `.width(...)` /
        // `.width_chars(...)` caps without wrapping the subtree
        // in a clipping `MaxSize` — the focus-state border stroke
        // can then extend 1 dp outside the visual bounds
        // (Int UI focus thickening) without being chopped off
        // against the shape quad.
        let effective_proposal = SizeProposal {
            width: match (proposal.width, self.pixel_cap) {
                (Some(w), Some(cap)) => Some(w.min(cap).max(self.min_width)),
                (None, Some(cap)) => Some(cap.max(self.min_width)),
                (w, None) => w,
            },
            height: proposal.height,
        };
        self.root_child_id
            .and_then(|id| ctx.child_size(id, effective_proposal))
            .unwrap_or_else(|| effective_proposal.resolve(0.0, 0.0))
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        if let Some(p) = children.first_mut() {
            p.origin = Point::new(bounds.x, bounds.y);
            p.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        use fern_core::accesskit::{Action, Role};

        builder.set_role(Role::SpinButton);
        if let Some(ref label) = self.label {
            builder.set_name(label);
        }
        builder.set_numeric_value(self.value.get().to_f64());
        builder.set_min_numeric_value(self.min.to_f64());
        builder.set_max_numeric_value(self.max.to_f64());
        builder.set_numeric_value_step(self.single_step.to_f64());
        if let Some(page) = self.page_step {
            builder.set_numeric_value_jump(page.to_f64());
        } else {
            builder.set_numeric_value_jump(
                self.single_step.saturating_mul_u32(10).to_f64(),
            );
        }
        // String-valued representation so screen readers can read
        // out the suffix / special-value text when applicable. The
        // suffix is elided when `special_value_text` has kicked in
        // (value == min), matching the visual rendering.
        let value = self.value.get();
        let using_special =
            self.special_value_text.is_some() && approx_eq(value, self.min);
        let display = format_for_display(
            value,
            self.decimals,
            self.special_value_text.as_deref(),
            self.text_from_value.as_deref(),
            self.min,
            false,
        );
        let full = if !self.suffix.is_empty() && !using_special {
            format!("{}{}", display, self.suffix)
        } else {
            display
        };
        builder.set_value(full);

        if !self.enabled {
            builder.set_disabled();
        }
        if self.read_only {
            builder.set_read_only();
        }
        builder.add_action(Action::Increment);
        builder.add_action(Action::Decrement);
        builder.add_action(Action::SetValue);
        builder.add_action(Action::Focus);
    }
}

// ── Helpers ────────────────────────────────────────────────────────

/// Build the stacked up/down step-button column. Returns its root
/// `WidgetId` for the caller to drop into the HStack.
///
/// `step` (with `EventContext`) is called on the initial tap;
/// `step_silent` is called from the hold-to-repeat timer, which
/// runs inside a frame-tick effect with no `EventContext` to hand.
fn build_buttons<T: SpinValue>(
    ctx: &mut BuildContext,
    step: &Rc<dyn Fn(i32, bool, &mut EventContext)>,
    step_silent: &Rc<dyn Fn(i32, bool) -> Option<T>>,
    can_up: Signal<bool>,
    can_down: Signal<bool>,
    enabled: bool,
    frame_height: f32,
    corner_radius: f32,
) -> WidgetId {
    // Each button is half of the field's inner height (minus the
    // borders and a 1 px gutter between the two).
    let button_height = ((frame_height - 2.0) * 0.5).max(8.0);
    let button_width = 18.0;

    let up_icon = chevron_up_icon(8.0);
    let down_icon = chevron_down_icon(8.0);

    // Derived enabled signals: OR with the caller-wide `enabled`.
    let up_enabled = if enabled { can_up } else { Signal::new(false) };
    let down_enabled = if enabled { can_down } else { Signal::new(false) };

    let step_for_up_tap = step.clone();
    let silent_for_up_auto = step_silent.clone();
    let up_button = StepButton::new(up_icon, up_enabled, move |ctx| {
        (step_for_up_tap)(1, false, ctx);
    })
    .on_auto_repeat(move || {
        (silent_for_up_auto)(1, false);
    })
    .size(button_width, button_height)
    .corner_radius(CornerRadius {
        top_left: 0.0,
        top_right: corner_radius,
        bottom_left: 0.0,
        bottom_right: 0.0,
    });

    let step_for_down_tap = step.clone();
    let silent_for_down_auto = step_silent.clone();
    let down_button = StepButton::new(down_icon, down_enabled, move |ctx| {
        (step_for_down_tap)(-1, false, ctx);
    })
    .on_auto_repeat(move || {
        (silent_for_down_auto)(-1, false);
    })
    .size(button_width, button_height)
    .corner_radius(CornerRadius {
        top_left: 0.0,
        top_right: 0.0,
        bottom_left: 0.0,
        bottom_right: corner_radius,
    });

    let up_id = ctx.add(up_button);
    let down_id = ctx.add(down_button);

    let column = VStack::new()
        .spacing(0.0)
        .add_child(up_id)
        .add_child(down_id);
    ctx.add(column)
}

/// Small chevron-up icon at `size` px. Mirrors the shape of the
/// `chevron_down` icon provided by `IconWidget`.
fn chevron_up_icon(size: f32) -> IconWidget {
    let mut path = Path::new();
    let s = size;
    path.move_to(Point::new(s * 0.25, s * 0.65));
    path.line_to(Point::new(s * 0.5, s * 0.35));
    path.line_to(Point::new(s * 0.75, s * 0.65));
    IconWidget::from_path(path, size)
}

fn chevron_down_icon(size: f32) -> IconWidget {
    let mut path = Path::new();
    let s = size;
    path.move_to(Point::new(s * 0.25, s * 0.35));
    path.line_to(Point::new(s * 0.5, s * 0.65));
    path.line_to(Point::new(s * 0.75, s * 0.35));
    IconWidget::from_path(path, size)
}

/// Format `value` for display, honoring `special_value_text` when
/// applicable and deferring to a user-supplied formatter when set.
///
/// `force_plain` bypasses `special_value_text` even when the value
/// equals `min` — used when the field is focused so the user can
/// edit the number instead of a placeholder string.
fn format_for_display<T: SpinValue>(
    value: T,
    decimals: u8,
    special: Option<&str>,
    custom: Option<&dyn Fn(T) -> String>,
    min: T,
    force_plain: bool,
) -> String {
    if !force_plain
        && let Some(special_text) = special
        && approx_eq(value, min)
    {
        return special_text.to_string();
    }
    match custom {
        Some(f) => f(value),
        None => value.format(decimals),
    }
}

/// Decide the effective step for an [`Adaptive`](StepType::Adaptive)
/// step type given the current value. For a value ∈ [10^n,
/// 10^(n+1)) the effective step is 10^n; inside [0, 1) the
/// step stays at `base_step` to avoid vanishing.
fn resolve_effective_step<T: SpinValue>(step_type: StepType, current: T, base_step: T) -> T {
    if step_type == StepType::Fixed {
        return base_step;
    }
    let abs = current.to_f64().abs();
    if abs < 10.0 {
        return base_step;
    }
    let pow = abs.log10().floor();
    let magnitude = 10f64.powf(pow);
    let adaptive = T::from_f64_saturating(magnitude);
    // Fall back to the user's base step if adaptive truncates to
    // zero (possible for integer types when pow < 0).
    let adaptive_f = adaptive.to_f64();
    if adaptive_f.abs() < 1e-12 {
        base_step
    } else {
        adaptive
    }
}

/// Approximate equality. Integer types compare bit-exactly;
/// floats tolerate sub-unit-in-last-place jitter. Used throughout
/// to suppress redundant signal sets.
fn approx_eq<T: SpinValue>(a: T, b: T) -> bool {
    if T::is_integer() {
        a.to_f64() == b.to_f64()
    } else {
        // Relative epsilon scaled by value magnitude so both near-zero
        // and large-value comparisons behave.
        let af = a.to_f64();
        let bf = b.to_f64();
        let scale = af.abs().max(bf.abs()).max(1.0);
        (af - bf).abs() <= scale * 1e-9
    }
}

fn approx_ne<T: SpinValue>(a: T, b: T) -> bool {
    !approx_eq(a, b)
}

/// Measure the advance width of `text` in logical pixels using the
/// app-wide `SharedTypesetter` (the same backend the field paints
/// with). Falls back to a rough heuristic when no typesetter is
/// installed (headless tests) so the caller still gets a non-zero
/// width and the `MaxSize` cap behaves reasonably.
fn measure_width_px(ctx: &mut BuildContext, text: &str, style: &TextStyle) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    if let Some(ts) = ctx.app_state::<SharedTypesetter>() {
        let backend = ts.as_text_backend();
        let layout = backend.borrow_mut().layout_single_line(text, style, None);
        return layout.width;
    }
    // Headless fallback: ~0.55 × font size per ASCII char is a
    // close approximation for Inter Regular at body weight.
    text.chars().count() as f32 * style.size * 0.55
}
