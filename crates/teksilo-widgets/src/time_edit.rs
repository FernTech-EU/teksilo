// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `TimeEdit` — text input for time-of-day, bound to `Signal<Option<Time>>`.
//!
//! Single-line editable time field with strftime-pattern parse/format
//! and optional 12h/24h mode + AM/PM. Same compositional pattern as
//! [`DateEdit`](crate::date_edit::DateEdit) (TextInputField + commit on
//! Enter/blur + step keys), without a popover (desktop convention is no
//! graphical time picker).
//!
//! # Behaviour
//!
//! - **Value binding**: `Signal<Option<Time>>` — `None` shows the
//!   placeholder.
//! - **Pattern**: 24h default `%H:%M`; 12h is `%I:%M %p`. Override
//!   via `format_pattern`. Add seconds with
//!   `seconds(SecondsMode::Editable)`.
//! - **Keyboard** (preview-pass on the wrapper):
//!   - Arrow Up / Down → ±`step_minutes`
//!   - PageUp / PageDown → ±60 minutes
//!   - Shift+ on either → ×10 multiplier (×600 max so values stay sane)
//!
//! # Accessibility
//!
//! - Container — `Role::TimeInput` with `set_value` formatted as
//!   `HH:MM:SS` and `set_label` from `.label()`.
//! - Underlying TextInputField keeps `Role::TextInput` so AT knows
//!   it's editable.
//!
//! ```ignore
//! use teksilo_core::signal::Signal;
//! use teksilo_widgets::time_edit::{TimeEdit, TimeFormat, SecondsMode};
//!
//! let value = Signal::new(None);
//! let _field = TimeEdit::new(value)
//!     .format(TimeFormat::Hour24)
//!     .seconds(SecondsMode::Hidden);
//! ```

#[cfg(test)]
mod tests;

use std::rc::Rc;

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accesskit::{Action, Role};
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::widget::{EventContext, LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::{localized, resolve_message_widget};

use crate::common::datetime::Time;
use crate::common::datetime::pattern::{
    ParseTarget, ParsedPattern, ParsedValue, format_value, mask_for_pattern, parse_value,
    segment_at_position, step_time_field,
};
use crate::date_edit::ValidationBehavior;
use crate::primitives::text_input_field::{ValidationFeedback, ValidationOutcome};
use crate::text_input::TextInput;
use teksilo_i18n::LocalizedString;

/// 12h vs 24h time formatting.
///
/// Used with [`TimeEdit::format`] to lock the clock style independently of the locale default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimeFormat {
    /// 24-hour clock (default — `%H:%M`).
    #[default]
    Hour24,
    /// 12-hour clock with AM/PM segment (`%I:%M %p`).
    Hour12,
}

/// Whether the seconds segment is shown in [`TimeEdit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SecondsMode {
    /// Hide the seconds segment (default).
    #[default]
    Hidden,
    /// Show and edit the seconds segment.
    Editable,
}

type OnValueChanged = Rc<dyn Fn(Option<Time>, &mut EventContext)>;

/// Single-line editable time-of-day field.
///
/// See the [module documentation](self) for full behaviour, pattern,
/// and keyboard details.
pub struct TimeEdit {
    value: Signal<Option<Time>>,
    /// Set by `::required(Signal<Time>)` — wired into `ctx.effect()`
    /// in `build()` so observer handles outlive construction.
    required_source: Option<Signal<Time>>,
    /// Explicit 12h/24h override. `None` (default) means "derive from
    /// the current locale" via `prefers_12_hour_clock`. Set via
    /// [`Self::format`] to lock a specific clock for the field.
    format: Option<TimeFormat>,
    seconds: SecondsMode,
    pattern_override: Option<String>,
    min_time: Option<Time>,
    max_time: Option<Time>,
    step_minutes: u32,
    placeholder: LocalizedString,
    /// Enabled state, static or reactive; forwarded to the arena and the
    /// inner `TextInput` at build time.
    enabled: Prop<bool>,
    read_only: bool,
    validation_behavior: ValidationBehavior,
    width_policy: crate::date_edit::WidthPolicy,
    label: Option<LocalizedString>,
    on_value_changed: Option<OnValueChanged>,
    text_signal: Signal<String>,
    focused: Signal<bool>,
    feedback: Signal<ValidationFeedback>,
    style_override: Option<teksilo_core::styles::SharedDateEditStyle>,
    root_child_id: Option<WidgetId>,
    /// Optional plain tooltip text shown after a hover delay. Mutually exclusive
    /// with the rich / composite slots — every setter clears the other two so
    /// the last call wins.
    tooltip_text: Option<LocalizedString>,
    /// Optional rich tooltip source (registry key or inline content).
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Optional composite tooltip body (arbitrary widget tree).
    composite_tooltip_content: Option<Box<dyn Widget>>,
}

impl std::fmt::Debug for TimeEdit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TimeEdit")
            .field("format", &self.format)
            .field("seconds", &self.seconds)
            .finish_non_exhaustive()
    }
}

impl TimeEdit {
    /// Construct bound to `value` (`None` = empty field; `Some(t)` = pre-filled time).
    pub fn new(value: Signal<Option<Time>>) -> Self {
        Self {
            value,
            required_source: None,
            format: None,
            seconds: SecondsMode::Hidden,
            pattern_override: None,
            min_time: None,
            max_time: None,
            step_minutes: 1,
            placeholder: LocalizedString::literal(String::new()),
            enabled: Prop::Static(true),
            read_only: false,
            validation_behavior: ValidationBehavior::AutoCorrect,
            width_policy: crate::date_edit::WidthPolicy::Default,
            label: None,
            on_value_changed: None,
            text_signal: Signal::new(String::new()),
            focused: Signal::new(false),
            feedback: Signal::new(ValidationFeedback::Pristine),
            style_override: None,
            root_child_id: None,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
        }
    }

    /// Per-call DateEditStyle override (shared with DateEdit family).
    pub fn style(mut self, style: impl teksilo_core::styles::DateEditStyle) -> Self {
        self.style_override = Some(std::rc::Rc::new(style));
        self
    }

    /// Construct with a **required** (non-nullable) `Signal<Time>`. The field
    /// never shows `None`; the signal and the internal `Option` are kept in sync.
    pub fn required(value: Signal<Time>) -> Self {
        let proxy: Signal<Option<Time>> = Signal::new(Some(value.get()));
        let mut s = Self::new(proxy);
        s.required_source = Some(value);
        s
    }

    /// Lock the field to a specific clock (12h or 24h). When this
    /// builder is *not* called, the field defaults to the user's
    /// current locale via `prefers_12_hour_clock` (12h for en-US /
    /// en-CA / en-AU / en-NZ / en-PH / en-IN / en-PK; 24h elsewhere).
    pub fn format(mut self, f: TimeFormat) -> Self {
        self.format = Some(f);
        self
    }

    /// Show or hide the seconds segment. Default: [`SecondsMode::Hidden`].
    pub fn seconds(mut self, mode: SecondsMode) -> Self {
        self.seconds = mode;
        self
    }

    /// Override the strftime-subset format pattern (e.g. `"%H:%M:%S"`).
    /// Bypasses the locale-derived and `format`-derived defaults entirely.
    pub fn format_pattern(mut self, p: impl Into<String>) -> Self {
        self.pattern_override = Some(p.into());
        self
    }

    /// Clamp the accepted value to at or after `t` (inclusive).
    pub fn min_time(mut self, t: Time) -> Self {
        self.min_time = Some(t);
        self
    }

    /// Clamp the accepted value to at or before `t` (inclusive).
    pub fn max_time(mut self, t: Time) -> Self {
        self.max_time = Some(t);
        self
    }

    /// Set the ArrowUp / ArrowDown step in minutes. Default: 1. Must be ≥ 1.
    pub fn step_minutes(mut self, n: u32) -> Self {
        self.step_minutes = n.max(1);
        self
    }

    /// Text shown when the field is empty (value is `None`).
    pub fn placeholder(mut self, text: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = text.into();
        self.placeholder = ls;
        self
    }

    /// Set the enabled state, statically or reactively. Forwarded to the
    /// arena at build time.
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
        self
    }

    /// Allow display-only mode: text is selectable but not editable.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// How parse failures are surfaced. See
    /// [`ValidationBehavior`].
    pub fn validation_behavior(mut self, behavior: ValidationBehavior) -> Self {
        self.validation_behavior = behavior;
        self
    }

    /// How the widget claims horizontal space. See
    /// [`WidthPolicy`](crate::date_edit::WidthPolicy). Default
    /// `Default` (natural mask-derived width).
    pub fn width_policy(mut self, policy: crate::date_edit::WidthPolicy) -> Self {
        self.width_policy = policy;
        self
    }

    /// Reactive handle on the live validation feedback.
    pub fn validation_feedback_signal(&self) -> Signal<ValidationFeedback> {
        self.feedback.clone()
    }

    /// Set the accessible label for the field (announced by screen readers).
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        self.label = Some(ls);
        self
    }

    /// Callback invoked on every committed value change with the new
    /// `Option<Time>` and a live `EventContext`.
    pub fn on_value_changed(
        mut self,
        f: impl Fn(Option<Time>, &mut EventContext) + 'static,
    ) -> Self {
        self.on_value_changed = Some(Rc::new(f));
        self
    }

    /// Attach a plain single-line tooltip shown after a hover delay. Clears
    /// any previously set rich or composite tooltip (last call wins).
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip identified by a registry key. Clears any
    /// previously set plain or composite tooltip (last call wins).
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach an inline rich tooltip from a [`crate::tooltip::TooltipContent`]
    /// value. Clears any previously set plain or composite tooltip (last call wins).
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip whose body is an arbitrary widget tree.
    /// Clears any previously set plain or rich tooltip (last call wins).
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }

    /// The bound value signal — the same `Signal` passed to [`Self::new`].
    pub fn value(&self) -> Signal<Option<Time>> {
        self.value.clone()
    }

    fn resolved_pattern(&self, format: TimeFormat) -> String {
        if let Some(p) = self.pattern_override.clone() {
            return p;
        }
        time_pattern_for(format, self.seconds)
    }
}

/// Resolve the strftime-subset pattern for the given clock + seconds
/// mode. `pub(crate)` so `DateTimeEdit` can share TimeEdit's pattern
/// derivation rules without duplicating the matcher.
pub(crate) fn time_pattern_for(format: TimeFormat, seconds: SecondsMode) -> String {
    match (format, seconds) {
        (TimeFormat::Hour24, SecondsMode::Hidden) => "%H:%M".into(),
        (TimeFormat::Hour24, SecondsMode::Editable) => "%H:%M:%S".into(),
        (TimeFormat::Hour12, SecondsMode::Hidden) => "%I:%M %p".into(),
        (TimeFormat::Hour12, SecondsMode::Editable) => "%I:%M:%S %p".into(),
    }
}

impl Widget for TimeEdit {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // required-source mirror via ctx.effect (see DateEdit::build
        // for the rationale — observers' RAII handles can't outlive
        // construction).
        if let Some(src) = self.required_source.clone() {
            {
                let proxy = self.value.clone();
                ctx.effect(&src, move |new| {
                    if proxy.get() != Some(*new) {
                        proxy.set(Some(*new));
                    }
                });
            }
            {
                let src_clone = src;
                ctx.effect(&self.value, move |v| {
                    if let Some(t) = v
                        && src_clone.get() != *t
                    {
                        src_clone.set(*t);
                    }
                });
            }
        }

        let self_id = ctx.self_id();
        // Forward the enabled state into the arena; see IconButton.
        ctx.enabled_when(self_id, self.enabled.clone());
        let enabled = self.enabled.clone();
        let read_only = self.read_only;

        // Resolve clock format: explicit override → locale default.
        let format = self.format.unwrap_or_else(|| {
            let tag = ctx.locale_signal().get().unwrap_or_default();
            if crate::common::datetime::prefers_12_hour_clock(&tag) {
                TimeFormat::Hour12
            } else {
                TimeFormat::Hour24
            }
        });
        let pattern_string = self.resolved_pattern(format);
        let parsed_pattern = ParsedPattern::parse(&pattern_string)
            .unwrap_or_else(|_| ParsedPattern::parse("%H:%M").unwrap());
        let pattern_rc = Rc::new(parsed_pattern);
        let on_value_changed = self.on_value_changed.clone();
        let min = self.min_time;
        let max = self.max_time;
        let step_minutes = self.step_minutes as i64;

        // Seed text from current value.
        {
            let init = match self.value.get() {
                Some(t) => format_value(&pattern_rc, None, Some(t)),
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
                    Some(t) => format_value(&pattern, None, Some(*t)),
                    None => String::new(),
                };
                if text_signal.get() != formatted {
                    text_signal.set(formatted);
                }
            });
        }

        // ── Validator ─────────────────────────────────────────
        // Mirrors DateEdit's design: pure classification; the
        // chained on_blur callback below re-parses and updates the
        // bound value signal.
        let validation_behavior = self.validation_behavior;
        let validator: crate::primitives::text_input_field::ValidatorFn = {
            let pattern = pattern_rc.clone();
            Rc::new(move |raw: &str| -> ValidationOutcome {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    return ValidationOutcome::Valid;
                }
                if let Some(ParsedValue::Time(t)) =
                    parse_value(&pattern, trimmed, ParseTarget::TimeOnly)
                {
                    let clamped = clamp_time(t, min, max);
                    let formatted = format_value(&pattern, None, Some(clamped));
                    if formatted == trimmed && clamped == t {
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
                if validation_behavior == ValidationBehavior::AutoCorrect
                    && let Some((corrected, msg)) =
                        try_clamp_time_recovery(&pattern, trimmed, min, max)
                {
                    return ValidationOutcome::Corrected {
                        corrected,
                        message: msg,
                    };
                }
                ValidationOutcome::Invalid {
                    message: localized(move || {
                        resolve_message_widget("time-edit-validation-not-a-time", &[])
                    }),
                }
            })
        };

        // Commit-side: read (now-corrected) text and update value.
        // Skips on Invalid so the user's typed text stays visible.
        let commit: Rc<dyn Fn(&mut EventContext)> = {
            let value_signal = self.value.clone();
            let text_signal = self.text_signal.clone();
            let feedback_signal = self.feedback.clone();
            let pattern = pattern_rc.clone();
            let on_value_changed = on_value_changed.clone();
            Rc::new(move |ctx_evt: &mut EventContext| {
                if matches!(feedback_signal.get(), ValidationFeedback::Invalid { .. }) {
                    return;
                }
                let raw = text_signal.get();
                let trimmed = raw.trim();
                let new_value: Option<Time> = if trimmed.is_empty() {
                    None
                } else {
                    match parse_value(&pattern, trimmed, ParseTarget::TimeOnly) {
                        Some(ParsedValue::Time(t)) => Some(clamp_time(t, min, max)),
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

        // No standalone ±minute-step closure — segment-aware
        // stepping replaces the pre-segment behaviour. The
        // `step_minutes` builder is kept on the public surface for
        // callers that pre-configured it (it now functions as a
        // hint for future per-segment custom steps; today the segment
        // step is always ±1 unit / ±10 with shift / ±10 / ±100 on
        // page keys).
        let _ = step_minutes;

        // ── TextInput composite ───────────────────────────────
        // Same trick as DateEdit: the framing, padding, validation
        // strip, and focus-driven border all live in TextInput. We
        // just pass the time-shaped configuration (pattern-derived
        // input mask, validator, char filter, commit handlers) and
        // wrap with a min-width floor so the field sits at the
        // editor's design width.
        let pattern_for_filter = pattern_rc.clone();
        let mask_string = mask_for_pattern(&pattern_rc);
        let mut text_input = TextInput::new(self.text_signal.clone())
            .placeholder(self.placeholder.clone())
            .enabled(enabled)
            .read_only(read_only)
            .input_mask(mask_string)
            .validator({
                let v = validator.clone();
                move |s| (v)(s)
            })
            .char_filter(move |c: char| {
                if c.is_ascii_digit() || c == ' ' || c == ':' {
                    return true;
                }
                if matches!(c, 'a' | 'A' | 'p' | 'P' | 'm' | 'M') {
                    return true;
                }
                for tok in &pattern_for_filter.tokens {
                    if let crate::common::datetime::pattern::PatternToken::Literal(s) = tok
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
        if let Some(label) = self.label.clone() {
            text_input = text_input.label(label);
        }

        let caret_for_step = text_input.caret_position();
        let caret_setter_for_step = text_input.caret_setter();

        {
            let inner_feedback = text_input.validation_feedback_signal();
            let outer_feedback = self.feedback.clone();
            ctx.effect(&inner_feedback, move |fb| {
                if outer_feedback.get() != *fb {
                    outer_feedback.set(fb.clone());
                }
            });
        }

        // Apply width policy. Default → natural mask-derived width.
        // Fill → wrap in intrinsic-respecting Expand so the field
        // stretches to its parent's offered width while still
        // reporting natural width when unconstrained.
        let body_id = match self.width_policy {
            crate::date_edit::WidthPolicy::Default => ctx.add(text_input),
            crate::date_edit::WidthPolicy::Fill => {
                let inner_id = ctx.add(text_input);
                ctx.add(
                    crate::primitives::Expand::horizontal()
                        .respect_intrinsic()
                        .child_id(inner_id),
                )
            }
        };
        let style = crate::styles::recipe_date_edit_style::resolve_date_edit_style(
            &self.style_override,
            ctx,
        );
        let cfg = teksilo_core::styles::DateEditStyleConfig { body: body_id };
        let root_id = style.make_body(&cfg, ctx);
        self.root_child_id = Some(root_id);

        // ── Tooltip attachment ────────────────────────────────
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

        // ── Segment-stepping helper ───────────────────────────
        let segment_step: Rc<dyn Fn(i32, &mut EventContext)> = {
            let pattern_for_step = pattern_rc.clone();
            let value_for_step = self.value.clone();
            let text_for_step = self.text_signal.clone();
            let on_changed_for_step = self.on_value_changed.clone();
            let min_for_step = self.min_time;
            let max_for_step = self.max_time;
            let caret_for_step = caret_for_step.clone();
            let caret_setter = caret_setter_for_step.clone();
            Rc::new(move |delta: i32, ctx_evt: &mut EventContext| {
                let caret = caret_for_step.get();
                let Some((_, _, kind)) = segment_at_position(&pattern_for_step, caret) else {
                    return;
                };
                let current = value_for_step.get().unwrap_or_else(Time::midnight);
                let stepped = step_time_field(current, kind, delta);
                let clamped = clamp_time(stepped, min_for_step, max_for_step);
                value_for_step.set(Some(clamped));
                text_for_step.set(format_value(&pattern_for_step, None, Some(clamped)));
                // Restore the caret — see the matching note in
                // `date_edit::DateEdit::build` for the rationale.
                caret_setter(caret);
                if let Some(cb) = on_changed_for_step.as_ref() {
                    cb(Some(clamped), ctx_evt);
                }
                ctx_evt.request_frame();
            })
        };

        // ── Self handlers: focus_within + segment-step keys ────
        // Self-attached `on_key_preview` claims arrow / page keys
        // BEFORE the focused field's `on_key`. Step targets the
        // segment under the caret (hour/minute/second/period). Shift
        // multiplies the unit by 10 for power-user sweeps.
        let step_for_key = segment_step.clone();
        let handlers = HandlerSet::new()
            .focus_within(self.focused.clone())
            .on_key_preview(move |event, ctx_evt| {
                // `enabled` gating is redundant here: a disabled TimeEdit's
                // arena-disabled state cascades to the focused inner field,
                // and `arena.is_enabled(target)` already gates the whole
                // preview dispatch before this closure runs. `read_only` has
                // no arena equivalent, so it still needs an explicit check.
                if read_only {
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

        // Bind value at AccessibilityOnly so the wrapper's set_value
        // refreshes when the bound time changes.
        let self_id = ctx.self_id();
        self.value.bind_to(
            self_id,
            ctx.binding_registry(),
            teksilo_core::binding::BindingLevel::AccessibilityOnly,
        );

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // Forward the full LayoutResponse from the inner widget so the
        // flex from `WidthPolicy::Fill`'s Expand wrapper survives. See
        // the matching note in `date_edit::DateEdit::layout_response`.
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
        builder.set_role(Role::TimeInput);
        if let Some(ref label) = self.label {
            builder.set_name(label.resolve_now());
        } else {
            builder.set_name(resolve_message_widget("time-edit-name", &[]));
        }
        match self.value.get() {
            Some(t) => {
                builder.set_value(format!(
                    "{:02}:{:02}:{:02}",
                    t.hour(),
                    t.minute(),
                    t.second()
                ));
            }
            None => {
                if !self.placeholder.resolve_now().is_empty() {
                    builder.set_placeholder(self.placeholder.resolve_now());
                } else {
                    builder.set_placeholder(resolve_message_widget("time-edit-placeholder", &[]));
                }
            }
        }
        // Framework a11y walker sets `set_disabled` from arena state.
        if self.read_only {
            builder.set_read_only();
        }
        builder.add_action(Action::Focus);
        // SetValue is advertised on the inner field (overridden to
        // Role::TimeInput). Wrapper duplicating it would route AT-
        // invoked SetValue through both nodes.
    }
}

pub(crate) fn clamp_time(t: Time, min: Option<Time>, max: Option<Time>) -> Time {
    let t = match min {
        Some(min) if t < min => min,
        _ => t,
    };
    match max {
        Some(max) if t > max => max,
        _ => t,
    }
}

/// Build a time-validator closure suitable for plugging into
/// `TextInputField::validator(...)`. Mirrors
/// [`crate::date_edit::build_date_validator`] in shape: lenient
/// strict-parse → clamp-recovery → reject. Used by `TimeEdit` itself
/// AND by `DateTimeEdit`'s time half so both share the same parsing
/// semantics without duplicating the closure body.
pub(crate) fn build_time_validator(
    pattern: Rc<ParsedPattern>,
    min: Option<Time>,
    max: Option<Time>,
    behavior: ValidationBehavior,
) -> crate::primitives::text_input_field::ValidatorFn {
    Rc::new(move |raw: &str| -> ValidationOutcome {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return ValidationOutcome::Valid;
        }
        if let Some(ParsedValue::Time(t)) = parse_value(&pattern, trimmed, ParseTarget::TimeOnly) {
            let clamped = clamp_time(t, min, max);
            let formatted = format_value(&pattern, None, Some(clamped));
            if formatted == trimmed && clamped == t {
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
            && let Some((corrected, msg)) = try_clamp_time_recovery(&pattern, trimmed, min, max)
        {
            return ValidationOutcome::Corrected {
                corrected,
                message: msg,
            };
        }
        ValidationOutcome::Invalid {
            message: localized(move || {
                resolve_message_widget("time-edit-validation-not-a-time", &[])
            }),
        }
    })
}

/// AutoCorrect recovery for time inputs. Walks the pattern, extracts
/// per-segment digit runs, clamps each value to its valid range
/// (hour → 0..=23 in 24h or 1..=12 in 12h, minute/second → 0..=59),
/// and re-constructs. The AM/PM segment is parsed permissively.
pub(crate) fn try_clamp_time_recovery(
    pattern: &ParsedPattern,
    raw: &str,
    min: Option<Time>,
    max: Option<Time>,
) -> Option<(String, LocalizedString)> {
    use crate::common::datetime::pattern::{PatternToken, SegmentKind};
    let mut cursor = raw;
    let mut hour24: Option<i8> = None;
    let mut hour12: Option<i8> = None;
    let mut minute: Option<i8> = None;
    let mut second: Option<i8> = None;
    let mut period: Option<i8> = None;
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
                    return None;
                }
            }
            PatternToken::Segment(kind) => {
                if matches!(kind, SegmentKind::Period) {
                    let first = cursor.chars().next()?;
                    let upper = first.to_ascii_uppercase();
                    let consumed = cursor.chars().next().map(|c| c.len_utf8()).unwrap_or(0);
                    let after_first = &cursor[consumed..];
                    let consumed2 = after_first
                        .chars()
                        .next()
                        .filter(|c| c.is_ascii_alphabetic())
                        .map(|c| c.len_utf8())
                        .unwrap_or(0);
                    cursor = &after_first[consumed2..];
                    period = Some(if upper == 'P' { 1 } else { 0 });
                    continue;
                }
                let max_d = kind.max_digits();
                if max_d == 0 {
                    continue;
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
                    return None;
                }
                let digits = &cursor[..end];
                cursor = &cursor[end..];
                let raw_v: i32 = digits.parse().ok()?;
                let (lo, hi) = kind.value_range().unwrap_or((i32::MIN, i32::MAX));
                let clamped = raw_v.clamp(lo, hi);
                if clamped != raw_v {
                    let segment_key = match kind {
                        SegmentKind::Hour24
                        | SegmentKind::Hour24Short
                        | SegmentKind::Hour12
                        | SegmentKind::Hour12Short => "validation-segment-hour",
                        SegmentKind::Minute | SegmentKind::MinuteShort => {
                            "validation-segment-minute"
                        }
                        SegmentKind::Second | SegmentKind::SecondShort => {
                            "validation-segment-second"
                        }
                        _ => "validation-segment-value",
                    };
                    let label = resolve_message_widget(segment_key, &[]);
                    clamp_notes.push(localized(move || {
                        resolve_message_widget(
                            "validation-segment-clamped",
                            &[
                                ("segment", label.clone().into()),
                                ("raw", (raw_v as i64).into()),
                                ("clamped", (clamped as i64).into()),
                            ],
                        )
                    }));
                }
                match kind {
                    SegmentKind::Hour24 | SegmentKind::Hour24Short => hour24 = Some(clamped as i8),
                    SegmentKind::Hour12 | SegmentKind::Hour12Short => hour12 = Some(clamped as i8),
                    SegmentKind::Minute | SegmentKind::MinuteShort => minute = Some(clamped as i8),
                    SegmentKind::Second | SegmentKind::SecondShort => second = Some(clamped as i8),
                    _ => {}
                }
            }
        }
    }

    let hour = match (hour24, hour12, period) {
        (Some(h), _, _) => h,
        (None, Some(h12), Some(p)) => (h12 % 12) + if p == 1 { 12 } else { 0 },
        (None, Some(h12), None) => h12 % 12,
        (None, None, _) => return None,
    };
    let t = Time::new(hour, minute.unwrap_or(0), second.unwrap_or(0), 0).ok()?;
    let final_t = clamp_time(t, min, max);
    if final_t != t {
        clamp_notes.push(localized(move || {
            resolve_message_widget("validation-clamped-to-range", &[])
        }));
    }
    let formatted = format_value(pattern, None, Some(final_t));
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
        // This requires a bit more work since we can't join LocalizedStrings directly.
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
