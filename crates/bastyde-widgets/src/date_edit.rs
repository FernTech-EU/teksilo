// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `DateEdit` — text input + calendar popover, bound to `Signal<Option<Date>>`.
//!
//! A single-line editable date field. The underlying surface is a
//! `TextInputField` displaying the formatted date; commit on Enter or
//! blur parses the input against the active pattern, clamps to
//! `[min_date, max_date]`, and writes the result back. A trailing
//! calendar-icon button opens a [`Calendar`]
//! popover anchored below the field for graphical date selection.
//!
//! # Behaviour
//!
//! - **Value binding**: `Signal<Option<Date>>` is the source of truth.
//!   External writes re-format the text. `None` shows the placeholder.
//! - **Pattern**: locale-derived strftime-subset (`%Y-%m-%d`,
//!   `%m/%d/%Y`, …); override via `format_pattern`.
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
//! use bastyde::widgets::{DateEdit, common::datetime::Date};
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

use bastyde_i18n::localized;
use std::rc::Rc;

use bastyde_canvas::{Path, Point, Rect, SizeProposal};
use bastyde_core::accessibility::{AccessNodeBuilder, widget_id_to_node_id};
use bastyde_core::accesskit::{Action, HasPopup, Role};
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::overlay::{
    DismissBehavior, OverlayDismissCallback, OverlayLayer, OverlayPlacement, OverlayRequest,
};
use bastyde_core::signal::{Prop, Signal};
use bastyde_core::widget::{EventContext, LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::resolve_message_widget;
use jiff::civil::Weekday;

use crate::calendar::Calendar;
use crate::common::datetime::Date;
use crate::common::datetime::pattern::{
    ParseTarget, ParsedPattern, ParsedValue, PatternToken, SegmentKind, format_value,
    mask_for_pattern, parse_value, segment_at_position, step_date_field,
};
use crate::common::datetime::types::{YearMonth, today_local};
use crate::icon_button::{IconButton, IconButtonSize};
use crate::primitives::IconWidget;
use crate::primitives::text_input_field::{ValidationFeedback, ValidationOutcome};
use crate::text_input::TextInput;
use bastyde_i18n::LocalizedString;

type OnValueChanged = Rc<dyn Fn(Option<Date>, &mut EventContext)>;

/// How a datetime widget claims horizontal space.
///
/// Shared across `DateEdit`, `TimeEdit`, `DateRangeEdit`, and
/// `DateTimeEdit`. For the two-half widgets the policy applies to
/// the *trailing* half only — the leading half always sizes to its
/// mask-derived natural width so the date never reflows when only
/// the time half changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WidthPolicy {
    /// **Default.** The widget claims its natural width: the mask-derived
    /// empty template (`__/__/____` for ISO date, `__:__` for 24h time)
    /// measured in the theme body font plus surrounding chrome.
    /// The footprint stays fixed as the user types — Int UI form-density
    /// convention. This is the [`Default`].
    #[default]
    Default,
    /// The widget expands to fill the horizontal space its parent offers,
    /// instead of capping at the natural mask width. Use inside toolbars,
    /// inspector panels, or an `Expand::horizontal` column that should
    /// stretch with the surrounding layout.
    Fill,
}

/// How the date editor reacts to out-of-range or partially invalid input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ValidationBehavior {
    /// Out-of-range inputs are clamped to the nearest valid value
    /// (e.g. `12/50/2026` → `12/31/2026`) and announced via `Live::Polite`.
    /// Matches macOS Calendar and iOS DatePicker. This is the [`Default`].
    #[default]
    AutoCorrect,
    /// Out-of-range inputs are rejected with an inline error message;
    /// the field's text is left as-typed so the user can correct it.
    /// The bound value is unchanged until a valid date is committed.
    /// Matches Excel / Material strict-validation patterns. Use for
    /// high-precision contexts where silently rounding is unacceptable.
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
    placeholder: LocalizedString,
    first_day_of_week: Option<Weekday>,
    show_calendar_button: bool,
    calendar_popover_placement: OverlayPlacement,
    /// Enabled state, static or reactive; forwarded to the arena at
    /// build time.
    enabled: Prop<bool>,
    read_only: bool,
    /// How parse failures are surfaced. Default `AutoCorrect`.
    validation_behavior: ValidationBehavior,
    /// How the field claims horizontal space. Default
    /// [`WidthPolicy::Default`] — the field sizes to its natural
    /// mask-derived width and stays put.
    width_policy: WidthPolicy,
    label: Option<LocalizedString>,
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
    /// Optional plain tooltip text shown after a hover delay. Mutually exclusive
    /// with the rich / composite slots — every setter clears the other two so
    /// the last call wins.
    tooltip_text: Option<LocalizedString>,
    /// Optional rich tooltip source (registry key or inline content).
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Optional composite tooltip body (arbitrary widget tree).
    composite_tooltip_content: Option<Box<dyn Widget>>,
    // Build state
    /// Per-call DateEditStyle override. Higher precedence than the
    /// theme-wide `style_slots.date_edit` slot.
    style_override: Option<bastyde_core::styles::SharedDateEditStyle>,
    root_child_id: Option<WidgetId>,
    calendar_id: Option<WidgetId>,
}

impl std::fmt::Debug for DateEdit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DateEdit")
            .field("min", &self.min_date)
            .field("max", &self.max_date)
            .field("enabled", &self.enabled.get())
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
            placeholder: LocalizedString::literal(String::new()),
            first_day_of_week: None,
            show_calendar_button: true,
            calendar_popover_placement: OverlayPlacement::BelowPreferred,
            enabled: Prop::Static(true),
            read_only: false,
            validation_behavior: ValidationBehavior::AutoCorrect,
            width_policy: WidthPolicy::Default,
            label: None,
            on_value_changed: None,
            feedback: Signal::new(ValidationFeedback::Pristine),
            text_signal: Signal::new(String::new()),
            focused: Signal::new(false),
            popover_open: Signal::new(false),
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            style_override: None,
            root_child_id: None,
            calendar_id: None,
        }
    }

    /// Per-call style override for the date-edit chrome.
    pub fn style(mut self, style: impl bastyde_core::styles::DateEditStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
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

    /// Clamp the selectable range from below. Dates earlier than `d`
    /// are rejected on commit and are shown as disabled in the calendar popover.
    pub fn min_date(mut self, d: Date) -> Self {
        self.min_date = Some(d);
        self
    }

    /// Clamp the selectable range from above. Dates later than `d`
    /// are rejected on commit and are shown as disabled in the calendar popover.
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

    /// Text displayed when the bound value is `None`. Defaults to empty
    /// (no placeholder rendered).
    pub fn placeholder(mut self, text: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = text.into();
        self.placeholder = ls;
        self
    }

    /// Override which weekday heads the calendar's column grid.
    /// Defaults to the locale's convention if not set.
    pub fn first_day_of_week(mut self, w: Weekday) -> Self {
        self.first_day_of_week = Some(w);
        self
    }

    /// Show or hide the trailing calendar-icon trigger button that opens
    /// the calendar popover. Default `true`.
    pub fn show_calendar_button(mut self, show: bool) -> Self {
        self.show_calendar_button = show;
        self
    }

    /// Override where the calendar popover appears relative to the field.
    /// Default is [`OverlayPlacement::BelowPreferred`].
    pub fn calendar_popover_placement(mut self, p: OverlayPlacement) -> Self {
        self.calendar_popover_placement = p;
        self
    }

    /// Set the enabled state, statically or reactively. Forwarded to
    /// the arena at build time.
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
        self
    }

    /// Make the field read-only: text is selectable and copyable but
    /// not editable, and step keys are suppressed.
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

    /// How the widget claims horizontal space. Default
    /// [`WidthPolicy::Default`] — the field sizes to its natural
    /// mask-derived width. Switch to [`WidthPolicy::Fill`] to make
    /// the field stretch to fill the parent's offered width
    /// (toolbar / inspector pattern).
    pub fn width_policy(mut self, policy: WidthPolicy) -> Self {
        self.width_policy = policy;
        self
    }

    /// Reactive handle on the live validation feedback (mirrored from
    /// the inner field). Composites that want to render their own
    /// feedback UI elsewhere can bind to this; the default
    /// `ValidationStrip` slot below the field uses it internally.
    pub fn validation_feedback_signal(&self) -> Signal<ValidationFeedback> {
        self.feedback.clone()
    }

    /// Set the accessible label for the field (also shown by any paired
    /// `FormLayout` label slot). Defaults to the localized "Date" string.
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        self.label = Some(ls);
        self
    }

    /// Register a callback fired on every committed value change with the
    /// new `Option<Date>` and a live `EventContext`. Fires only on
    /// user-driven commits (typing + blur, Enter, calendar selection),
    /// not on external writes to the bound signal.
    pub fn on_value_changed(
        mut self,
        f: impl Fn(Option<Date>, &mut EventContext) + 'static,
    ) -> Self {
        self.on_value_changed = Some(Rc::new(f));
        self
    }

    /// Return a clone of the bound value signal for external observation.
    pub fn value(&self) -> Signal<Option<Date>> {
        self.value.clone()
    }

    /// Attach a plain single-line tooltip shown after a hover delay.
    /// Mutually exclusive with [`Self::rich_tooltip`],
    /// [`Self::rich_tooltip_content`], and [`Self::composite_tooltip`] —
    /// this call clears those slots.
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip looked up by registry key. Mutually exclusive
    /// with [`Self::tooltip`], [`Self::rich_tooltip_content`], and
    /// [`Self::composite_tooltip`] — this call clears those slots.
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip from inline content. Mutually exclusive with
    /// [`Self::tooltip`], [`Self::rich_tooltip`], and
    /// [`Self::composite_tooltip`] — this call clears those slots.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip whose body is an arbitrary widget tree.
    /// Mutually exclusive with [`Self::tooltip`], [`Self::rich_tooltip`],
    /// and [`Self::rich_tooltip_content`] — this call clears those slots.
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
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
                    if let Some(d) = v
                        && src_clone.get() != *d
                    {
                        src_clone.set(*d);
                    }
                });
            }
        }

        let theme = ctx.theme_signal().get();
        use crate::styles::recipe_date_edit_style as de;
        let _ = &theme;
        let self_id = ctx.self_id();
        // Forward the enabled state into the arena; see IconButton.
        ctx.enabled_when(self_id, self.enabled.clone());
        let enabled = self.enabled.get();
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
                        message: localized(move || {
                            resolve_message_widget(
                                "validation-corrected-to",
                                &[("value", formatted.clone().into())],
                            )
                        }),
                    };
                }
                // 2. Strict parse failed. Try clamp-recovery: extract
                //    each segment value, clamp out-of-range values to
                //    their valid range, and re-construct.
                if validation_behavior == ValidationBehavior::AutoCorrect
                    && let Some((corrected, msg)) = try_clamp_recovery(&pattern, trimmed, min, max)
                {
                    return ValidationOutcome::Corrected {
                        corrected,
                        message: msg,
                    };
                }
                // 3. Truly unparseable. Reject.
                ValidationOutcome::Invalid {
                    message: localized(move || {
                        resolve_message_widget("date-edit-validation-not-a-date", &[])
                    }),
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

        // ── Calendar popover (pre-built dormant) ──────────────
        // Built before the TextInput composite so the trailing-slot
        // trigger button can capture the calendar's id.
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
            let mut calendar =
                Calendar::single(calendar_temp.clone()).on_activate(move |d, ctx_evt| {
                    let clamped = clamp_date(d, min, max);
                    value_for_cal.set(Some(clamped));
                    text_signal_for_cal.set(format_value(&pattern_for_cal, Some(clamped), None));
                    if let Some(cb) = on_value_changed_for_cal.as_ref() {
                        cb(Some(clamped), ctx_evt);
                    }
                    popover_open.set(false);
                    ctx_evt.dismiss_self_overlay_chain();
                    // Return focus to the DateEdit so keyboard users
                    // are back at the trigger after committing —
                    // matches the open path's `request_focus(calendar_id)`
                    // and keeps the focus pointer on a sensible widget
                    // (Tab from here lands wherever Tab would have
                    // gone next, not at the document root).
                    ctx_evt.request_focus(return_focus_to);
                    ctx_evt.request_frame();
                });
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

        // ── Calendar trigger button (built as a value, dropped into
        //    the TextInput's trailing slot) ──────────────────────
        // Same Int UI `IconButton` (in embedded mode) the other
        // datetime widgets (DateRangeEdit, DateTimeEdit) use, so the
        // visual treatment — hover/pressed background, icon size,
        // focus halo — stays consistent across the family.
        let trigger_widget_opt: Option<IconButton> = if self.show_calendar_button {
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
            Some(
                IconButton::new(calendar_glyph_icon(de::CALENDAR_ICON_SIZE))
                    .embedded()
                    .size(IconButtonSize::Default)
                    .enabled(enabled && !read_only)
                    .tooltip(localized(move || {
                        resolve_message_widget("date-edit-trigger-tooltip", &[])
                    }))
                    .on_activate_fn(move |ctx_evt: &mut EventContext| {
                        if popover_open.get() {
                            popover_open.set(false);
                            ctx_evt.dismiss_all_except_hosts();
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
            )
        } else {
            None
        };

        // ── TextInput composite ───────────────────────────────
        // Drops the date-shaped editing surface into the same frame
        // every TextInput uses (border, padding, validation strip,
        // focus border) and parks the calendar trigger in its
        // trailing slot — flush against the field's right edge with
        // no manual divider, matching Int UI's embedded IconButton convention.
        let pattern_for_filter = pattern_rc.clone();
        let mask_string = mask_for_pattern(&pattern_rc);
        let mut text_input = TextInput::new(self.text_signal.clone())
            .placeholder(placeholder.clone())
            .enabled(enabled)
            .read_only(read_only)
            .input_mask(mask_string)
            .validator({
                let v = validator.clone();
                move |s| (v)(s)
            })
            .char_filter(move |c: char| {
                if c.is_ascii_digit() || c == '-' || c == ' ' {
                    return true;
                }
                for tok in &pattern_for_filter.tokens {
                    if let PatternToken::Literal(s) = tok
                        && s.chars().any(|x| x == c)
                    {
                        return true;
                    }
                }
                false
            })
            .on_submit_fn({
                let commit = commit.clone();
                move |ctx_evt| commit(ctx_evt)
            })
            .on_blur_fn({
                let commit = commit.clone();
                move |ctx_evt| commit(ctx_evt)
            });
        // NB (audit G9): the label is intentionally NOT forwarded to the inner
        // TextInput. DateEdit's own accessibility() node (Role::DateInput)
        // already carries the name; naming the inner TextInput too would both
        // double-label AND give its GenericContainer semantic content, which
        // stops the AT walker from dropping it as a presentational node — the
        // exact cause of the redundant middle node. With no name the container
        // is content-free and collapses, leaving the 2-node tree
        // DateEdit(DateInput) -> TextInputField(TextInput + character runs),
        // matching the SpinBox shape.
        if let Some(trigger) = trigger_widget_opt {
            text_input = text_input.trailing_slot(trigger);
        }

        // Capture caret signal AND a caret setter BEFORE moving the
        // composite into the tree. The setter is a no-op until
        // `build()` populates the slot; segment_step uses it to
        // restore the caret AFTER rewriting text.
        let caret_for_step = text_input.caret_position();
        let caret_setter_for_step = text_input.caret_setter();

        // Mirror the inner field's published feedback into our own
        // signal so the commit closure (which short-circuits on
        // Invalid) reads the live state. The TextInput composite also
        // wires this internally to its ValidationStrip.
        {
            let inner_feedback = text_input.validation_feedback_signal();
            let outer_feedback = self.feedback.clone();
            ctx.effect(&inner_feedback, move |fb| {
                if outer_feedback.get() != *fb {
                    outer_feedback.set(fb.clone());
                }
            });
        }

        // Apply width policy. `Default` adds nothing — the field
        // reports its natural mask-derived width via TextInputField.
        // `Fill` wraps in an intrinsic-respecting Expand so the
        // composite stretches to its parent's offered width while
        // still reporting the natural width when unconstrained
        // (matches SpinBox's `.fill_width()` semantics).
        let body_id = match self.width_policy {
            WidthPolicy::Default => ctx.add(text_input),
            WidthPolicy::Fill => {
                let inner_id = ctx.add(text_input);
                ctx.add(
                    crate::primitives::Expand::horizontal()
                        .respect_intrinsic()
                        .child_id(inner_id),
                )
            }
        };
        // Delegate any final wrapping to the active DateEditStyle.
        let style = crate::styles::recipe_date_edit_style::resolve_date_edit_style(
            &self.style_override,
            ctx,
        );
        let cfg = bastyde_core::styles::DateEditStyleConfig { body: body_id };
        let root_id = style.make_body(&cfg, ctx);
        self.root_child_id = Some(root_id);

        // ── Tooltip attachment ─────────────────────────────────
        // Anchored on the visible trigger root (not the calendar overlay).
        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, root_id, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, root_id, source, delay);
        } else if let Some(text) = self.tooltip_text.clone() {
            let tooltip_widget = crate::tooltip::TooltipWidget::new(text);
            let tooltip_id = ctx.add(tooltip_widget);
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip(root_id, tooltip_id, delay);
        }

        // ── Segment-stepping helper — captured by the on_key_preview
        // self handler below. Reads live caret position, looks up the
        // segment under the caret, and applies a single field step
        // (year / month / day / hour / minute / second / period).
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
                let Some((_, _, kind)) = segment_at_position(&pattern_for_step, caret) else {
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
            bastyde_core::binding::BindingLevel::AccessibilityOnly,
        );
        self.popover_open.bind_to(
            self_id,
            registry,
            bastyde_core::binding::BindingLevel::AccessibilityOnly,
        );

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        // Forward the full LayoutResponse — including flex — from the
        // child. When `WidthPolicy::Fill` is active, the inner Expand
        // wrapper reports flex=1; without forwarding it here, parent
        // HStacks see flex=0 and the field never grows.
        match self.root_child_id {
            Some(id) => ctx
                .child_layout_response(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into()),
            None => proposal.resolve(0.0, 0.0).into(),
        }
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
            builder.set_name(label.resolve_now());
        } else {
            builder.set_name(resolve_message_widget("date-edit-name", &[]));
        }
        match self.value.get() {
            Some(d) => {
                builder.set_value(format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day()));
            }
            None => {
                if !self.placeholder.resolve_now().is_empty() {
                    builder.set_placeholder(self.placeholder.resolve_now());
                } else {
                    builder.set_placeholder(resolve_message_widget("date-edit-placeholder", &[]));
                }
            }
        }
        // Framework a11y walker sets `set_disabled` from arena state.
        if self.read_only {
            builder.set_read_only();
        }
        builder.add_action(Action::Focus);
        // SetValue isn't advertised on this DateInput node because the inner
        // TextInputField (Role::TextInput) handles text entry via its own
        // TextInputField semantics; routing through both nodes would
        // double-process AT requests. The intermediate TextInput
        // GenericContainer is dropped by the presentational-node collapse
        // (its label is no longer forwarded — see build()), so the AT tree is
        // exactly DateInput -> TextInput(editable) with its character runs.
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

pub(crate) fn clamp_date(d: Date, min: Option<Date>, max: Option<Date>) -> Date {
    let d = match min {
        Some(min) if d < min => min,
        _ => d,
    };
    match max {
        Some(max) if d > max => max,
        _ => d,
    }
}

/// Build a date-validator closure suitable for plugging into
/// `TextInputField::validator(...)`. Encapsulates the strict-parse →
/// clamp-recovery → reject pipeline that `DateEdit` itself uses,
/// so other widgets composing a `TextInputField` over a date pattern
/// (e.g. `DateRangeEdit`'s start / end halves) can reuse the same
/// validation behaviour without duplicating ~50 lines.
///
/// `pattern` and `behavior` are captured by value; `min`/`max` clamp
/// the parsed date when present.
pub(crate) fn build_date_validator(
    pattern: Rc<ParsedPattern>,
    min: Option<Date>,
    max: Option<Date>,
    behavior: ValidationBehavior,
) -> crate::primitives::text_input_field::ValidatorFn {
    Rc::new(move |raw: &str| -> ValidationOutcome {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return ValidationOutcome::Valid;
        }
        if let Some(ParsedValue::Date(d)) = parse_value(&pattern, trimmed, ParseTarget::DateOnly) {
            let clamped = clamp_date(d, min, max);
            let formatted = format_value(&pattern, Some(clamped), None);
            if formatted == trimmed && clamped == d {
                return ValidationOutcome::Valid;
            }
            return ValidationOutcome::Corrected {
                corrected: formatted.clone(),
                message: localized(move || {
                    resolve_message_widget(
                        "validation-corrected-to",
                        &[("value", formatted.clone().into())],
                    )
                }),
            };
        }
        if behavior == ValidationBehavior::AutoCorrect
            && let Some((corrected, msg)) = try_clamp_recovery(&pattern, trimmed, min, max)
        {
            return ValidationOutcome::Corrected {
                corrected,
                message: msg,
            };
        }
        ValidationOutcome::Invalid {
            message: localized(move || {
                resolve_message_widget("date-edit-validation-not-a-date", &[])
            }),
        }
    })
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
pub(crate) fn try_clamp_recovery(
    pattern: &ParsedPattern,
    raw: &str,
    min: Option<Date>,
    max: Option<Date>,
) -> Option<(String, LocalizedString)> {
    // Walk the pattern; for each digit segment, take whatever digit
    // run starts at the current cursor position. For literal tokens,
    // optionally consume the literal (lenient — same logic as
    // parse_value's literal handling).
    let mut cursor = raw;
    let mut year: Option<i16> = None;
    let mut month: Option<i8> = None;
    let mut day: Option<i8> = None;
    let mut clamp_notes: Vec<LocalizedString> = Vec::new();

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
                        SegmentKind::Month | SegmentKind::MonthShort => "validation-segment-month",
                        SegmentKind::Day | SegmentKind::DayShort => "validation-segment-day",
                        _ => "validation-segment-value",
                    };
                    let segment_label = resolve_message_widget(segment_key, &[]);
                    clamp_notes.push(localized(move || {
                        resolve_message_widget(
                            "validation-segment-clamped",
                            &[
                                ("segment", segment_label.clone().into()),
                                ("raw", (raw_v as i64).into()),
                                ("clamped", (clamped as i64).into()),
                            ],
                        )
                    }));
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
        clamp_notes.push(localized(move || {
            resolve_message_widget(
                "validation-day-clamped-to-month",
                &[
                    ("raw", (raw_day as i64).into()),
                    ("clamped", (d as i64).into()),
                ],
            )
        }));
    }

    let date = Date::new(y, m, d).ok()?;
    let final_date = clamp_date(date, min, max);
    if final_date != date {
        clamp_notes.push(localized(move || {
            resolve_message_widget("validation-clamped-to-range", &[])
        }));
    }

    let formatted = format_value(pattern, Some(final_date), None);
    let formatted_for_msg = formatted.clone();
    let message = if clamp_notes.is_empty() {
        localized(move || {
            resolve_message_widget(
                "validation-corrected-to",
                &[("value", formatted_for_msg.clone().into())],
            )
        })
    } else {
        // For the notes case, we need to resolve all notes and join them.
        // We'll resolve them at display time.
        localized(move || {
            let notes_str: String = clamp_notes
                .iter()
                .map(|n| n.resolve_now())
                .collect::<Vec<_>>()
                .join(", ");
            resolve_message_widget(
                "validation-corrected-with-notes",
                &[("notes", notes_str.into())],
            )
        })
    };
    Some((formatted, message))
}
