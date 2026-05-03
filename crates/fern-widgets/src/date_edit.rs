//! `DateEdit` — text input + calendar popover, bound to `Signal<Option<Date>>`.
//!
//! A single-line editable date field. The underlying surface is a
//! `TextInputField` displaying the formatted date; commit on Enter or
//! blur parses the input against the active pattern, clamps to
//! `[min_date, max_date]`, and writes the result back. A trailing
//! calendar-icon button opens a [`Calendar`](crate::calendar::Calendar)
//! popover anchored below the field for graphical date selection.
//!
//! # Behaviour
//!
//! - **Value binding**: `Signal<Option<Date>>` is the source of truth.
//!   External writes re-format the text. `None` shows the placeholder.
//! - **Pattern**: locale-derived strftime-subset (`%Y-%m-%d`,
//!   `%m/%d/%Y`, …); override via [`format_pattern`](Self::format_pattern).
//! - **Step keys** (preview-pass on the field):
//!   - Arrow Up / Down → ±1 day; Shift+ → ±7 days.
//!   - Page Up / Page Down → ±1 month; Shift+ → ±1 year.
//!   - `Alt+ArrowDown` (or click the calendar icon) → opens calendar
//!     popover.
//! - **Calendar popover**: dismisses on click-outside or Escape,
//!   commits on cell click, animates with `motion.duration_fast` fade.
//! - **Min / Max**: clamps on commit and on step. Out-of-range values
//!   in the popover cell are disabled.
//!
//! # Accessibility
//!
//! - Container — `Role::DateInput`, `set_value` to ISO selection,
//!   `set_label` from `.label()` builder, `set_placeholder` when
//!   value is `None`.
//! - Calendar trigger button — `Role::Button` with
//!   `set_has_popup(HasPopup::Grid)` and `set_expanded(open)`.
//! - Internally the editing surface remains a `Role::TextInput` for
//!   AT discoverability (so screen readers know it accepts text); the
//!   wrapper carries the DateInput role on the outer node.
//!
//! # Example
//!
//! ```ignore
//! use fern_ui::widgets::{DateEdit, common::datetime::Date};
//!
//! let date = ctx.signal(Some(Date::constant(2026, 5, 2)));
//! ctx.add(
//!     DateEdit::new(date.clone())
//!         .min_date(Date::constant(2020, 1, 1))
//!         .max_date(Date::constant(2030, 12, 31))
//!         .label("Birth date"),
//! );
//! ```

#[cfg(test)]
mod tests;

use std::rc::Rc;

use fern_canvas::{Path, Point, Rect, Size, SizeProposal};
use fern_core::accessibility::{widget_id_to_node_id, AccessNodeBuilder};
use fern_core::accesskit::{Action, HasPopup, Role};
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::overlay::{
    DismissBehavior, OverlayDismissCallback, OverlayLayer, OverlayPlacement, OverlayRequest,
};
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::{HandlerSet, WidgetBuilder};
use fern_core::widget_id::WidgetId;
use fern_i18n::resolve_message_widget;
use fern_tokens::{BorderRole, CornerRadius, SurfaceRole};
use jiff::civil::Weekday;

use crate::calendar::Calendar;
use crate::common::datetime::pattern::{
    format_value, mask_for_pattern, parse_value, segment_at_position, step_date_field,
    ParseTarget, ParsedPattern, ParsedValue, PatternToken, SegmentKind,
};
use crate::common::datetime::types::{today_local, YearMonth};
use crate::common::datetime::Date;
use crate::primitives::text_input_field::{ValidationFeedback, ValidationOutcome};
use crate::primitives::{
    Center, Divider, FixedSize, HStack, IconWidget, MinSize, Padding, RectWidget, ZStack,
};
use crate::primitives::text_input_field::TextInputField;

const DEFAULT_WIDTH: f32 = 144.0;

type OnValueChanged = Rc<dyn Fn(Option<Date>, &mut EventContext)>;

/// How the date editor reacts to out-of-range input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ValidationBehavior {
    /// **Default.** Out-of-range inputs are clamped to the nearest
    /// valid value (e.g. `12/50/2026` → `12/31/2026`) and announced
    /// via `Live::Polite`. Matches macOS Calendar / iOS DatePicker.
    #[default]
    AutoCorrect,
    /// Out-of-range inputs are rejected with an error message and
    /// the field's text is left as-typed (so the user can fix it).
    /// The bound value is unchanged. Matches Excel / Material strict
    /// validation. Use for high-precision contexts.
    Reject,
}

/// Single-line date input with optional calendar popover. See the
/// [module docs](self) for the full feature list.
pub struct DateEdit {
    value: Signal<Option<Date>>,
    /// Set by `::required(Signal<Date>)` — the original non-nullable
    /// upstream that needs to mirror with `value`. Wired via
    /// `ctx.effect()` in `build()` so the observer handles live with
    /// the widget rather than being dropped at construction.
    required_source: Option<Signal<Date>>,
    min_date: Option<Date>,
    max_date: Option<Date>,
    pattern: Option<String>,
    placeholder: String,
    first_day_of_week: Option<Weekday>,
    show_calendar_button: bool,
    calendar_popover_placement: OverlayPlacement,
    enabled: bool,
    read_only: bool,
    /// How parse failures are surfaced. Default `AutoCorrect`.
    validation_behavior: ValidationBehavior,
    label: Option<String>,
    on_value_changed: Option<OnValueChanged>,
    /// Live feedback signal mirrored from the inner field, owned by
    /// `DateEdit` so the wrapper's `accessibility()` and the
    /// `ValidationStrip` below the field both bind to it.
    feedback: Signal<ValidationFeedback>,
    /// Live edit text driven by both user typing and programmatic
    /// re-formatting (mirroring SpinBox's pattern).
    text_signal: Signal<String>,
    /// Field-focus tracker; controls whether the value reformat effect
    /// stomps on user typing.
    focused: Signal<bool>,
    /// Whether the calendar popover is currently open. Drives
    /// `set_expanded` on the trigger.
    popover_open: Signal<bool>,
    // Build state
    root_child_id: Option<WidgetId>,
    field_id: Option<WidgetId>,
    calendar_id: Option<WidgetId>,
    trigger_id: Option<WidgetId>,
}

impl std::fmt::Debug for DateEdit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DateEdit")
            .field("min", &self.min_date)
            .field("max", &self.max_date)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl DateEdit {
    /// Construct a date editor bound to a nullable date signal.
    pub fn new(value: Signal<Option<Date>>) -> Self {
        Self {
            value,
            required_source: None,
            min_date: None,
            max_date: None,
            pattern: None,
            placeholder: String::new(),
            first_day_of_week: None,
            show_calendar_button: true,
            calendar_popover_placement: OverlayPlacement::BelowPreferred,
            enabled: true,
            read_only: false,
            validation_behavior: ValidationBehavior::AutoCorrect,
            label: None,
            on_value_changed: None,
            feedback: Signal::new(ValidationFeedback::Pristine),
            text_signal: Signal::new(String::new()),
            focused: Signal::new(false),
            popover_open: Signal::new(false),
            root_child_id: None,
            field_id: None,
            calendar_id: None,
            trigger_id: None,
        }
    }

    /// Construct from a non-nullable date signal. Internally backed by
    /// a `Signal<Option<Date>>` proxy that mirrors the source in both
    /// directions. The placeholder is unused — the proxy is always
    /// initialized to `Some(value.get())` and the mirror keeps it
    /// non-empty.
    pub fn required(value: Signal<Date>) -> Self {
        let proxy: Signal<Option<Date>> = Signal::new(Some(value.get()));
        let mut s = Self::new(proxy);
        s.required_source = Some(value);
        s
    }

    pub fn min_date(mut self, d: Date) -> Self {
        self.min_date = Some(d);
        self
    }

    pub fn max_date(mut self, d: Date) -> Self {
        self.max_date = Some(d);
        self
    }

    /// Override the locale-derived format pattern (strftime subset, see
    /// `crate::common::datetime::pattern`).
    pub fn format_pattern(mut self, pat: impl Into<String>) -> Self {
        self.pattern = Some(pat.into());
        self
    }

    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    pub fn first_day_of_week(mut self, w: Weekday) -> Self {
        self.first_day_of_week = Some(w);
        self
    }

    pub fn show_calendar_button(mut self, show: bool) -> Self {
        self.show_calendar_button = show;
        self
    }

    pub fn calendar_popover_placement(mut self, p: OverlayPlacement) -> Self {
        self.calendar_popover_placement = p;
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

    /// How parse failures are surfaced. Default
    /// [`ValidationBehavior::AutoCorrect`] (clamp + announce); switch
    /// to [`ValidationBehavior::Reject`] for strict-validation form
    /// contexts.
    pub fn validation_behavior(mut self, behavior: ValidationBehavior) -> Self {
        self.validation_behavior = behavior;
        self
    }

    /// Reactive handle on the live validation feedback (mirrored from
    /// the inner field). Composites that want to render their own
    /// feedback UI elsewhere can bind to this; the default
    /// `ValidationStrip` slot below the field uses it internally.
    pub fn validation_feedback_signal(&self) -> Signal<ValidationFeedback> {
        self.feedback.clone()
    }

    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }

    pub fn on_value_changed(
        mut self,
        f: impl Fn(Option<Date>, &mut EventContext) + 'static,
    ) -> Self {
        self.on_value_changed = Some(Rc::new(f));
        self
    }

    pub fn value(&self) -> Signal<Option<Date>> {
        self.value.clone()
    }
}

impl Widget for DateEdit {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Wire required-source mirror via ctx.effect so the observer
        // handles live with the widget rather than being dropped at
        // construction. Effects auto-clean on rebuild.
        if let Some(src) = self.required_source.clone() {
            // Source → proxy.
            {
                let proxy = self.value.clone();
                ctx.effect(&src, move |new| {
                    if proxy.get() != Some(*new) {
                        proxy.set(Some(*new));
                    }
                });
            }
            // Proxy → source. The proxy can hold `None` transiently
            // (parse failure → cleared text); ignore that and let the
            // next valid commit re-establish the value. The required
            // contract is "always have a value upstream", which the
            // initial seed in `::required` guarantees.
            {
                let src_clone = src;
                ctx.effect(&self.value, move |v| {
                    if let Some(d) = v {
                        if src_clone.get() != *d {
                            src_clone.set(*d);
                        }
                    }
                });
            }
        }

        let theme = ctx.theme_signal().get();
        let field_style = theme.components.text_field;
        let date_style = theme.components.date_edit;
        let focus_ring_width = theme.shape.focus_ring_width;
        let enabled = self.enabled;
        let read_only = self.read_only;

        // Resolve pattern: explicit override → locale default.
        let pattern_string = self.pattern.clone().unwrap_or_else(|| {
            let tag = ctx.locale_signal().get().unwrap_or_default();
            crate::common::datetime::format_pattern_for_locale(&tag).to_string()
        });
        let parsed_pattern = ParsedPattern::parse(&pattern_string)
            .unwrap_or_else(|_| ParsedPattern::parse("%Y-%m-%d").unwrap());
        let pattern_rc = Rc::new(parsed_pattern);
        let placeholder = self.placeholder.clone();
        let min = self.min_date;
        let max = self.max_date;
        let on_value_changed = self.on_value_changed.clone();

        // Seed text from current value.
        {
            let init = match self.value.get() {
                Some(d) => format_value(&pattern_rc, Some(d), None),
                None => String::new(),
            };
            self.text_signal.set(init);
        }

        // External writes → reformat (skip while focused).
        {
            let text_signal = self.text_signal.clone();
            let focused = self.focused.clone();
            let pattern = pattern_rc.clone();
            ctx.effect(&self.value, move |new_value| {
                if focused.get() {
                    return;
                }
                let formatted = match new_value {
                    Some(d) => format_value(&pattern, Some(*d), None),
                    None => String::new(),
                };
                if text_signal.get() != formatted {
                    text_signal.set(formatted);
                }
            });
        }

        // ── Validator ─────────────────────────────────────────
        // Pure classification: given raw text, return one of three
        // outcomes. The field's wrapper writes feedback signal +
        // re-formats text on `Corrected`. The on_blur callback
        // (chained AFTER the validator) re-parses the (now-corrected)
        // text and updates the bound value signal + fires the user
        // callback with EventContext.
        let validation_behavior = self.validation_behavior;
        let validator: crate::primitives::text_input_field::ValidatorFn = {
            let pattern = pattern_rc.clone();
            Rc::new(move |raw: &str| -> ValidationOutcome {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    // Empty is valid (clears value to None on commit).
                    return ValidationOutcome::Valid;
                }
                // 1. Try strict parse + reformat-compare to detect
                //    lenient-fill normalization (e.g., "2026" →
                //    "2026-01-01", or "2026-5" → "2026-05-01").
                if let Some(ParsedValue::Date(d)) =
                    parse_value(&pattern, trimmed, ParseTarget::DateOnly)
                {
                    let clamped = clamp_date(d, min, max);
                    let formatted = format_value(&pattern, Some(clamped), None);
                    if formatted == trimmed && clamped == d {
                        return ValidationOutcome::Valid;
                    }
                    return ValidationOutcome::Corrected {
                        corrected: formatted.clone(),
                        message: resolve_message_widget(
                            "validation-corrected-to",
                            &[("value", formatted.clone().into())],
                        ),
                    };
                }
                // 2. Strict parse failed. Try clamp-recovery: extract
                //    each segment value, clamp out-of-range values to
                //    their valid range, and re-construct.
                if validation_behavior == ValidationBehavior::AutoCorrect {
                    if let Some((corrected, msg)) =
                        try_clamp_recovery(&pattern, trimmed, min, max)
                    {
                        return ValidationOutcome::Corrected {
                            corrected,
                            message: msg,
                        };
                    }
                }
                // 3. Truly unparseable. Reject.
                ValidationOutcome::Invalid {
                    message: resolve_message_widget(
                        "date-edit-validation-not-a-date",
                        &[],
                    ),
                }
            })
        };

        // Commit-side effect: when the inner field's wrapper writes
        // a Corrected outcome, the text_signal already holds the
        // formatted-corrected text. Re-parse and sync the bound
        // value + fire on_value_changed via the chained on_blur
        // callback below.
        //
        // For Invalid: leave the typed text in the field so the user
        // can fix it; do NOT silently revert (the user's complaint
        // that triggered this whole feature). The bound value stays
        // unchanged.
        let commit: Rc<dyn Fn(&mut EventContext)> = {
            let value_signal = self.value.clone();
            let text_signal = self.text_signal.clone();
            let feedback_signal = self.feedback.clone();
            let pattern = pattern_rc.clone();
            let on_value_changed = on_value_changed.clone();
            Rc::new(move |ctx_evt: &mut EventContext| {
                let fb = feedback_signal.get();
                if matches!(fb, ValidationFeedback::Invalid { .. }) {
                    // Don't touch value or reformat text; let the
                    // user fix what they typed.
                    return;
                }
                let raw = text_signal.get();
                let trimmed = raw.trim();
                let new_value: Option<Date> = if trimmed.is_empty() {
                    None
                } else {
                    match parse_value(&pattern, trimmed, ParseTarget::DateOnly) {
                        Some(ParsedValue::Date(d)) => Some(clamp_date(d, min, max)),
                        _ => value_signal.get(),
                    }
                };
                if value_signal.get() != new_value {
                    value_signal.set(new_value);
                    if let Some(cb) = on_value_changed.as_ref() {
                        cb(new_value, ctx_evt);
                    }
                }
            })
        };

        // No standalone day-step closure — segment-aware stepping is
        // installed inside the on_key_preview self handler below
        // (replaces the pre-segment ±day stepping that used to live
        // here).

        // ── Inner editing field ───────────────────────────────
        let inner_height = (field_style.height - 2.0 * field_style.border_width).max(0.0);
        let text_area_height = (inner_height - 2.0 * field_style.padding_vertical).max(0.0);

        let pattern_for_filter = pattern_rc.clone();
        let access_label_for_field = self.label.clone();
        // Auto-derive an input mask from the pattern: gives the empty
        // field a `__/__/____` template + per-position char filtering.
        let mask_string = mask_for_pattern(&pattern_rc);
        let mut field = TextInputField::new(self.text_signal.clone())
            .enabled(enabled)
            .read_only(read_only)
            .placeholder(placeholder.clone())
            .text_height(text_area_height)
            .input_mask(&mask_string)
            .validator({
                let v = validator.clone();
                move |s| (v)(s)
            })
            .char_filter(move |c: char| {
                // Accept digits + any literal char that appears in the pattern
                // tokens (separators) + leading minus for negative years.
                if c.is_ascii_digit() || c == '-' || c == ' ' {
                    return true;
                }
                for tok in &pattern_for_filter.tokens {
                    if let PatternToken::Literal(s) = tok {
                        if s.chars().any(|x| x == c) {
                            return true;
                        }
                    }
                }
                false
            });
        // Mirror the field's published feedback into our own signal
        // so the wrapper's accessibility() and the ValidationStrip
        // both bind to the same source.
        {
            let inner_feedback = field.validation_feedback_signal();
            let outer_feedback = self.feedback.clone();
            ctx.effect(&inner_feedback, move |fb| {
                if outer_feedback.get() != *fb {
                    outer_feedback.set(fb.clone());
                }
            });
        }
        // Submit on Enter — commit + keep focus.
        {
            let commit = commit.clone();
            field = field.on_submit_fn(move |ctx_evt| commit(ctx_evt));
        }
        // Commit on blur.
        {
            let commit = commit.clone();
            field = field.on_blur_fn(move |ctx_evt| commit(ctx_evt));
        }

        // Capture caret signal AND a caret setter BEFORE moving the
        // field into the tree. The setter is a no-op until `build()`
        // populates the field's state slot; segment_step uses it to
        // restore the caret AFTER rewriting text (otherwise
        // `cursor.insert_text` parks the caret at the document end).
        let caret_for_step = field.caret_position();
        let caret_setter_for_step = field.caret_setter();

        // A11y override on the field: AT users who tab into the
        // editable surface hear "Date input" rather than "Edit text".
        // The field's existing TextInput-flavored AT (text content,
        // selection, character lengths, word starts) all stays — only
        // the role + label slot are overridden by the access_* layer.
        let mut field_with_a11y = field.access_role(Role::DateInput);
        if let Some(label) = access_label_for_field {
            field_with_a11y = field_with_a11y.access_label(label);
        }
        let field_id = ctx.add(field_with_a11y);
        self.field_id = Some(field_id);

        // ── Segment-stepping helper — captured by the on_key_preview
        // self handler below. Reads live caret position, looks up the
        // segment under the caret, and applies a single field step
        // (year / month / day / hour / minute / second / period).
        // Wrapped in `Rc` so the closure is reusable from the self-
        // attached HandlerSet without capturing-and-moving once.
        let segment_step: Rc<dyn Fn(i32, &mut EventContext)> = {
            let pattern_for_step = pattern_rc.clone();
            let value_for_step = self.value.clone();
            let text_for_step = self.text_signal.clone();
            let on_changed_for_step = on_value_changed.clone();
            let min_for_step = self.min_date;
            let max_for_step = self.max_date;
            let caret_for_step = caret_for_step.clone();
            let caret_setter = caret_setter_for_step.clone();
            Rc::new(move |delta: i32, ctx_evt: &mut EventContext| {
                let caret = caret_for_step.get();
                let Some((_, _, kind)) = segment_at_position(&pattern_for_step, caret)
                else {
                    return;
                };
                let current = value_for_step.get().unwrap_or_else(today_local);
                let stepped = step_date_field(current, kind, delta);
                let clamped = clamp_date(stepped, min_for_step, max_for_step);
                value_for_step.set(Some(clamped));
                text_for_step.set(format_value(&pattern_for_step, Some(clamped), None));
                // Restore the caret to where it was — `text_signal.set`
                // → field text effect → `cursor.insert_text` parked the
                // caret at the document end. Without this restore the
                // user has to re-click the segment between every Up/Down.
                caret_setter(caret);
                if let Some(cb) = on_changed_for_step.as_ref() {
                    cb(Some(clamped), ctx_evt);
                }
                ctx_evt.request_frame();
            })
        };

        let padded_field_id = ctx.add(
            Padding::new(
                field_style.padding_vertical,
                0.0,
                field_style.padding_vertical,
                0.0,
            )
            .child_id(field_id),
        );
        let expanded_field_id = ctx.add(
            crate::primitives::Expand::horizontal().child_id(padded_field_id),
        );

        // ── Calendar popover (pre-built dormant) ──────────────
        let calendar_id_opt = if self.show_calendar_button {
            // Bridge signal: the calendar binds to a parallel
            // `Signal<Option<Date>>` so its internal cell-render +
            // arrow-key state can mutate freely; the popover commit
            // path writes the final selection into our `value`. We
            // keep the bridge in sync with external `value` changes
            // via `ctx.effect` (NOT `observe()` — observers return
            // RAII handles that get dropped at construction; effects
            // live with the widget).
            let calendar_temp: Signal<Option<Date>> = Signal::new(self.value.get());
            {
                let temp = calendar_temp.clone();
                ctx.effect(&self.value, move |new_value| {
                    if temp.get() != *new_value {
                        temp.set(*new_value);
                    }
                });
            }
            let popover_open = self.popover_open.clone();
            let value_for_cal = self.value.clone();
            let text_signal_for_cal = self.text_signal.clone();
            let pattern_for_cal = pattern_rc.clone();
            let on_value_changed_for_cal = on_value_changed.clone();
            let return_focus_to = ctx.self_id();
            let mut calendar = Calendar::single(calendar_temp.clone()).on_activate(
                move |d, ctx_evt| {
                    let clamped = clamp_date(d, min, max);
                    value_for_cal.set(Some(clamped));
                    text_signal_for_cal.set(format_value(&pattern_for_cal, Some(clamped), None));
                    if let Some(cb) = on_value_changed_for_cal.as_ref() {
                        cb(Some(clamped), ctx_evt);
                    }
                    popover_open.set(false);
                    ctx_evt.dismiss_all_overlays();
                    // Return focus to the DateEdit so keyboard users
                    // are back at the trigger after committing —
                    // matches the open path's `request_focus(calendar_id)`
                    // and keeps the focus pointer on a sensible widget
                    // (Tab from here lands wherever Tab would have
                    // gone next, not at the document root).
                    ctx_evt.request_focus(return_focus_to);
                    ctx_evt.request_frame();
                },
            );
            if let Some(min) = min {
                calendar = calendar.min_date(min);
            }
            if let Some(max) = max {
                calendar = calendar.max_date(max);
            }
            if let Some(fdow) = self.first_day_of_week {
                calendar = calendar.first_day_of_week(fdow);
            }
            let calendar_id = ctx.add(calendar);
            ctx.set_dormant(calendar_id);
            Some(calendar_id)
        } else {
            None
        };
        self.calendar_id = calendar_id_opt;

        // ── Calendar trigger button ────────────────────────────
        let trigger_id_opt = if self.show_calendar_button {
            let popover_open = self.popover_open.clone();
            let calendar_id = calendar_id_opt.expect("calendar built when button enabled");
            let placement = self.calendar_popover_placement.clone();
            let self_ref = ctx.self_id();
            let dismiss_cb: OverlayDismissCallback = {
                let popover_open = popover_open.clone();
                Rc::new(move || {
                    popover_open.set(false);
                })
            };
            let trigger_widget = CalendarTriggerButton::new(
                date_style.calendar_button_width,
                date_style.calendar_icon_size,
                enabled && !read_only,
                Rc::new(move |ctx_evt: &mut EventContext| {
                    if popover_open.get() {
                        popover_open.set(false);
                        ctx_evt.dismiss_all_overlays();
                    } else {
                        popover_open.set(true);
                        ctx_evt.activate(calendar_id);
                        ctx_evt.show_overlay(OverlayRequest {
                            content_id: calendar_id,
                            anchor: self_ref,
                            placement: placement.clone(),
                            dismiss: DismissBehavior::EscapeOrClickOutside,
                            layer: OverlayLayer::InTree,
                            parent_overlay: None,
                            on_dismiss: Some(dismiss_cb.clone()),
                            fade_duration: None,
                        });
                        // Move focus into the calendar so arrow keys
                        // navigate cells immediately — standard date-
                        // picker UX (macOS Calendar, JetBrains, etc.).
                        // Without this the user must Tab through
                        // unrelated widgets first.
                        ctx_evt.request_focus(calendar_id);
                    }
                }),
            );
            Some(ctx.add(trigger_widget))
        } else {
            None
        };
        self.trigger_id = trigger_id_opt;

        // ── Row: field | divider | trigger ────────────────────
        let row_id = {
            let mut row = HStack::new().spacing(0.0);
            row = row.add_child(expanded_field_id);
            if let Some(trigger_id) = trigger_id_opt {
                let divider = Divider::vertical()
                    .thickness(1.0)
                    .color(BorderRole::Default);
                let divider_id = ctx.add(Padding::new(2.0, 0.0, 2.0, 0.0).child(divider));
                row = row.add_child(divider_id).add_child(trigger_id);
            }
            ctx.add(row)
        };
        let padded_row_id = ctx.add(
            Padding::new(
                0.0,
                field_style.padding_horizontal,
                0.0,
                field_style.padding_horizontal,
            )
            .child_id(row_id),
        );

        // ── Frame: focus-driven border ────────────────────────
        let border_role = self.focused.map(|f| {
            if *f {
                BorderRole::Focused
            } else {
                BorderRole::Default
            }
        });
        let border_width_signal = self.focused.map(move |f| {
            if *f {
                focus_ring_width
            } else {
                field_style.border_width
            }
        });
        let bg = RectWidget::new()
            .background(SurfaceRole::Content)
            .border_color(border_role)
            .border_width(border_width_signal)
            .corner_radius(CornerRadius::uniform(field_style.corner_radius));
        let bg_id = ctx.add(bg);
        let zstack_id = ctx.add(ZStack::new().add_child(bg_id).add_child(padded_row_id));

        let sized_id = ctx.add(MinSize::new(DEFAULT_WIDTH, field_style.height).child_id(zstack_id));

        // ── Inline validation strip ───────────────────────────
        // Wraps the field + strip in a VStack so the strip sits
        // directly below the frame. The strip reports zero size when
        // feedback is Pristine/Valid, so the wrapper collapses cleanly
        // when there's no message to show.
        let strip_id = ctx.add(crate::primitives::ValidationStrip::new(self.feedback.clone()));
        let root_with_strip = ctx.add(
            crate::primitives::VStack::new()
                .spacing(field_style.validation_strip_gap)
                .add_child(sized_id)
                .add_child(strip_id),
        );
        self.root_child_id = Some(root_with_strip);

        // ── Self handlers: focus_within + segment-step keys ────
        // `on_key_preview` on SELF (DateEdit, an actual ancestor of
        // the inner field) claims ArrowUp/ArrowDown/PageUp/PageDown
        // BEFORE the focused field's `on_key` runs. The step targets
        // the segment under the caret (year/month/day) — Qt-style
        // segment-stepping. Shift multiplies the unit step by 10 so
        // power users can sweep faster (e.g. ±10 years on the year
        // segment).
        let step_for_key = segment_step.clone();
        let handlers = HandlerSet::new()
            .focus_within(self.focused.clone())
            .on_key_preview(move |event, ctx_evt| {
                if !enabled || read_only {
                    return EventResponse::Ignored;
                }
                let WidgetEvent::KeyDown { key, modifiers, .. } = event else {
                    return EventResponse::Ignored;
                };
                let mult = if modifiers.shift() { 10 } else { 1 };
                let delta = match key {
                    Key::ArrowUp => mult,
                    Key::ArrowDown => -mult,
                    Key::PageUp => 10 * mult,
                    Key::PageDown => -10 * mult,
                    _ => return EventResponse::Ignored,
                };
                step_for_key(delta, ctx_evt);
                EventResponse::Handled
            });
        ctx.apply_self_handlers(handlers);

        // Bind reactive sources at AccessibilityOnly so the wrapper's
        // AT node refreshes its `value` and `set_expanded` whenever
        // the underlying signals change. Without these, the
        // wrapper's accessibility() never re-runs after a value
        // change and AT users hear stale data.
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.value.bind_to(
            self_id,
            registry,
            fern_core::binding::BindingLevel::AccessibilityOnly,
        );
        self.popover_open.bind_to(
            self_id,
            registry,
            fern_core::binding::BindingLevel::AccessibilityOnly,
        );

        vec![root_with_strip]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        match self.root_child_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0)),
            None => proposal.resolve(0.0, 0.0),
        }
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::DateInput);
        if let Some(ref label) = self.label {
            builder.set_name(label);
        } else {
            builder.set_name(resolve_message_widget("date-edit-name", &[]));
        }
        match self.value.get() {
            Some(d) => {
                builder.set_value(format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day()));
            }
            None => {
                if !self.placeholder.is_empty() {
                    builder.set_placeholder(self.placeholder.clone());
                } else {
                    builder.set_placeholder(resolve_message_widget("date-edit-placeholder", &[]));
                }
            }
        }
        if !self.enabled {
            builder.set_disabled();
        }
        if self.read_only {
            builder.set_read_only();
        }
        builder.add_action(Action::Focus);
        // SetValue isn't advertised on the wrapper because the inner
        // field (overridden to Role::DateInput) handles it via
        // TextInputField semantics. Routing through both nodes would
        // double-process AT requests.
        builder.set_has_popup(HasPopup::Grid);
        builder.set_expanded(self.popover_open.get());
        // Wire popup-controlled relationship when the calendar
        // exists. Pointing only when open caused stale ids on the
        // first frame after open; safe to point when closed too —
        // the calendar widget remains in the arena (dormant) and
        // its NodeId is valid.
        if let Some(cal_id) = self.calendar_id {
            builder.push_controlled(widget_id_to_node_id(cal_id));
        }
    }
}

// ── Calendar trigger button (icon-only, in-frame) ─────────────────────

pub(crate) struct CalendarTriggerButton {
    width: f32,
    icon_size: f32,
    enabled: bool,
    on_activate: Rc<dyn Fn(&mut EventContext)>,
    root_id: Option<WidgetId>,
}

impl std::fmt::Debug for CalendarTriggerButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CalendarTriggerButton")
            .field("width", &self.width)
            .finish()
    }
}

impl CalendarTriggerButton {
    pub(crate) fn new(width: f32, icon_size: f32, enabled: bool, on_activate: Rc<dyn Fn(&mut EventContext)>) -> Self {
        Self {
            width,
            icon_size,
            enabled,
            on_activate,
            root_id: None,
        }
    }
}

impl Widget for CalendarTriggerButton {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let icon = ctx.add(calendar_glyph_icon(self.icon_size));
        let centered = ctx.add(Center::new().child_id(icon));
        let bg = ctx.add(
            RectWidget::new()
                .background(SurfaceRole::Transparent)
                .corner_radius(CornerRadius::uniform(2.0)),
        );
        let z = ctx.add(ZStack::new().add_child(bg).add_child(centered));
        let sized = ctx.add(
            FixedSize::new()
                .bind_width(self.width)
                .bind_height(self.width)
                .child_id(z),
        );

        let tap_action = self.on_activate.clone();
        let key_action = self.on_activate.clone();
        let access_action = self.on_activate.clone();
        let enabled = self.enabled;
        let handlers = HandlerSet::new()
            .focusable(enabled)
            .cursor(if enabled { CursorIcon::Pointer } else { CursorIcon::Default })
            .on_tap(move |_pos, ctx_evt| {
                if enabled {
                    tap_action(ctx_evt);
                }
            })
            .on_key(move |event, ctx_evt| {
                if !enabled {
                    return EventResponse::Ignored;
                }
                if let WidgetEvent::KeyDown { key, .. } = event {
                    if matches!(key, Key::Enter | Key::Space) {
                        key_action(ctx_evt);
                        return EventResponse::Handled;
                    }
                }
                EventResponse::Ignored
            })
            .on_access_action(move |action, ctx_evt| {
                if !enabled {
                    return EventResponse::Ignored;
                }
                if matches!(action, Action::Click) {
                    access_action(ctx_evt);
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            });
        ctx.apply_self_handlers(handlers);

        self.root_id = Some(sized);
        vec![sized]
    }

    fn layout_response(
        &self,
        _proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        match self.root_id {
            Some(id) => ctx
                .child_size(id, SizeProposal::unspecified())
                .unwrap_or_else(|| Size::new(self.width, self.width)),
            None => Size::new(self.width, self.width),
        }
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_id.into_iter().collect()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::Button);
        builder.set_name(resolve_message_widget("date-edit-calendar-button", &[]));
        builder.set_has_popup(HasPopup::Grid);
        if self.enabled {
            builder.add_action(Action::Click);
            builder.add_action(Action::Focus);
        } else {
            builder.set_disabled();
        }
    }
}

pub(crate) fn calendar_glyph_icon(size: f32) -> IconWidget {
    let mut path = Path::new();
    let s = size;
    // Outer rounded rectangle suggesting a calendar.
    let m = s * 0.1;
    path.move_to(Point::new(m, s * 0.25));
    path.line_to(Point::new(s - m, s * 0.25));
    path.line_to(Point::new(s - m, s - m));
    path.line_to(Point::new(m, s - m));
    path.close();
    // Top binding stripe.
    path.move_to(Point::new(m, s * 0.25));
    path.line_to(Point::new(s - m, s * 0.25));
    path.line_to(Point::new(s - m, s * 0.4));
    path.line_to(Point::new(m, s * 0.4));
    path.close();
    // Two binding rings.
    let ring_y_top = s * 0.1;
    let ring_y_bot = s * 0.3;
    let ring_w = s * 0.06;
    let ring1_x = s * 0.25;
    let ring2_x = s * 0.65;
    path.move_to(Point::new(ring1_x, ring_y_top));
    path.line_to(Point::new(ring1_x + ring_w, ring_y_top));
    path.line_to(Point::new(ring1_x + ring_w, ring_y_bot));
    path.line_to(Point::new(ring1_x, ring_y_bot));
    path.close();
    path.move_to(Point::new(ring2_x, ring_y_top));
    path.line_to(Point::new(ring2_x + ring_w, ring_y_top));
    path.line_to(Point::new(ring2_x + ring_w, ring_y_bot));
    path.line_to(Point::new(ring2_x, ring_y_bot));
    path.close();
    IconWidget::from_path(path, size)
}

fn clamp_date(d: Date, min: Option<Date>, max: Option<Date>) -> Date {
    let d = match min {
        Some(min) if d < min => min,
        _ => d,
    };
    match max {
        Some(max) if d > max => max,
        _ => d,
    }
}

/// AutoCorrect recovery: extract per-segment integer values from the
/// raw input by walking the pattern, clamp each to its valid range
/// (year as-is within jiff's bounds; month → 1..=12; day → 1..=
/// days_in_month for the resulting year/month), and re-construct.
///
/// Returns `Some((formatted, message))` on successful recovery,
/// `None` if the input is too malformed (e.g., contains non-digits at
/// digit positions or doesn't have enough segments).
///
/// Examples (pattern `%d/%m/%Y`):
/// - `"12/50/2026"` → `Some(("12/12/2026", "Auto-corrected: month 50 → 12"))`
///   (month clamped to its max 12)
/// - `"31/2/2024"` → `Some(("29/02/2024", "Auto-corrected: day 31 → 29 (last day of February)"))`
///   (day clamped to month length)
/// - `"abc"` → `None`
fn try_clamp_recovery(
    pattern: &ParsedPattern,
    raw: &str,
    min: Option<Date>,
    max: Option<Date>,
) -> Option<(String, String)> {
    // Walk the pattern; for each digit segment, take whatever digit
    // run starts at the current cursor position. For literal tokens,
    // optionally consume the literal (lenient — same logic as
    // parse_value's literal handling).
    let mut cursor = raw;
    let mut year: Option<i16> = None;
    let mut month: Option<i8> = None;
    let mut day: Option<i8> = None;
    let mut clamp_notes: Vec<String> = Vec::new();

    for token in &pattern.tokens {
        if cursor.is_empty() {
            break;
        }
        match token {
            PatternToken::Literal(lit) => {
                if let Some(rest) = cursor.strip_prefix(lit.as_str()) {
                    cursor = rest;
                } else if lit.starts_with(cursor) {
                    cursor = "";
                } else {
                    // Literal doesn't match → can't recover, give up.
                    return None;
                }
            }
            PatternToken::Segment(kind) => {
                let max_d = kind.max_digits();
                if max_d == 0 {
                    continue; // Period segments not handled here
                }
                let mut end = 0usize;
                for (i, ch) in cursor.char_indices() {
                    if ch.is_ascii_digit() && end < max_d {
                        end = i + ch.len_utf8();
                    } else {
                        break;
                    }
                }
                if end == 0 {
                    // No digits where we expected them; bail.
                    return None;
                }
                let digits = &cursor[..end];
                cursor = &cursor[end..];
                let raw_v: i32 = digits.parse().ok()?;
                let (lo, hi) = kind.value_range().unwrap_or((i32::MIN, i32::MAX));
                let clamped = raw_v.clamp(lo, hi);
                if clamped != raw_v {
                    let segment_key = match kind {
                        SegmentKind::Year => "validation-segment-year",
                        SegmentKind::Month | SegmentKind::MonthShort => {
                            "validation-segment-month"
                        }
                        SegmentKind::Day | SegmentKind::DayShort => {
                            "validation-segment-day"
                        }
                        _ => "validation-segment-value",
                    };
                    let segment_label = resolve_message_widget(segment_key, &[]);
                    clamp_notes.push(resolve_message_widget(
                        "validation-segment-clamped",
                        &[
                            ("segment", segment_label.into()),
                            ("raw", (raw_v as i64).into()),
                            ("clamped", (clamped as i64).into()),
                        ],
                    ));
                }
                match kind {
                    SegmentKind::Year => year = Some(clamped as i16),
                    SegmentKind::Month | SegmentKind::MonthShort => month = Some(clamped as i8),
                    SegmentKind::Day | SegmentKind::DayShort => day = Some(clamped as i8),
                    _ => {}
                }
            }
        }
    }

    let y = year?;
    let m = month.unwrap_or(1);
    // Day: clamp to days-in-month for the resolved (y, m). This
    // catches "31 February" → "28/29 February" (depends on leap).
    let last_day = YearMonth::new(y, m).last_day().day();
    let raw_day = day.unwrap_or(1);
    let d = raw_day.min(last_day).max(1);
    if d != raw_day {
        clamp_notes.push(resolve_message_widget(
            "validation-day-clamped-to-month",
            &[
                ("raw", (raw_day as i64).into()),
                ("clamped", (d as i64).into()),
            ],
        ));
    }

    let date = Date::new(y, m, d).ok()?;
    let final_date = clamp_date(date, min, max);
    if final_date != date {
        clamp_notes.push(resolve_message_widget(
            "validation-clamped-to-range",
            &[],
        ));
    }

    let formatted = format_value(pattern, Some(final_date), None);
    let message = if clamp_notes.is_empty() {
        resolve_message_widget(
            "validation-corrected-to",
            &[("value", formatted.clone().into())],
        )
    } else {
        resolve_message_widget(
            "validation-corrected-with-notes",
            &[("notes", clamp_notes.join(", ").into())],
        )
    };
    Some((formatted, message))
}
