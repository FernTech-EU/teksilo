//! `DateTimeEdit` — composes [`DateEdit`] + [`TimeEdit`] over a single
//! `Signal<Option<DateTime>>`.
//!
//! The widget owns two intermediate signals (date half + time half) that
//! mirror the bound `DateTime` value. Mutations on either half compose
//! back into the outer signal via internal effects; external writes to
//! the outer signal split into the two halves.
//!
//! # Behaviour
//!
//! - **Value binding**: `Signal<Option<DateTime>>` source of truth. The
//!   widget guarantees that whenever date and time halves both carry a
//!   value the outer signal carries the combined `DateTime`; if either
//!   half is `None` the outer signal is `None`.
//! - **Visual**: an `HStack` of `DateEdit` and `TimeEdit` separated by
//!   the configurable `separator` text. The two child widgets carry
//!   their own borders today (a follow-up task is to share one frame —
//!   tracked in the plan file under "Phase B follow-up").
//!
//! # Accessibility
//!
//! - Container — `Role::DateTimeInput` with `set_value` formatted as
//!   `YYYY-MM-DDTHH:MM:SS`.
//! - The composed `DateEdit` and `TimeEdit` keep their own roles
//!   (`DateInput`, `TimeInput`) as nested AT children.

#[cfg(test)]
mod tests;

use std::rc::Rc;

use fern_canvas::{Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::accesskit::{Action, Role};
use fern_core::build_context::BuildContext;
use fern_core::overlay::OverlayPlacement;
use fern_core::signal::Signal;
use fern_core::widget::{EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_i18n::resolve_message_widget;
use jiff::civil::Weekday;

use crate::common::datetime::{Date, DateTime, Time};
use crate::date_edit::{DateEdit, ValidationBehavior};
use crate::primitives::text_input_field::ValidationFeedback;
use crate::primitives::{HStack, TextWidget};
use crate::time_edit::{SecondsMode, TimeEdit, TimeFormat};

type OnValueChanged = Rc<dyn Fn(Option<DateTime>, &mut EventContext)>;

pub struct DateTimeEdit {
    value: Signal<Option<DateTime>>,
    date_part: Signal<Option<Date>>,
    time_part: Signal<Option<Time>>,
    /// Set by `::required(Signal<DateTime>)` — wired into `ctx.effect`
    /// in `build()` so observer handles outlive construction.
    required_source: Option<Signal<DateTime>>,
    date_format_pattern: Option<String>,
    time_format: TimeFormat,
    seconds: SecondsMode,
    min: Option<DateTime>,
    max: Option<DateTime>,
    step_minutes: u32,
    first_day_of_week: Option<Weekday>,
    show_calendar_button: bool,
    separator: String,
    placeholder: String,
    enabled: bool,
    read_only: bool,
    label: Option<String>,
    validation_behavior: ValidationBehavior,
    /// Composed validation feedback. Mirrors whichever half is more
    /// severe (Invalid > Corrected > Valid > Pristine) so external
    /// observers see a single signal. Each child editor still renders
    /// its own `ValidationStrip` underneath itself.
    feedback: Signal<ValidationFeedback>,
    on_value_changed: Option<OnValueChanged>,
    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for DateTimeEdit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DateTimeEdit").finish_non_exhaustive()
    }
}

impl DateTimeEdit {
    pub fn new(value: Signal<Option<DateTime>>) -> Self {
        let initial = value.get();
        let date_part = Signal::new(initial.map(|dt| dt.date()));
        let time_part = Signal::new(initial.map(|dt| dt.time()));
        Self {
            value,
            date_part,
            time_part,
            required_source: None,
            date_format_pattern: None,
            time_format: TimeFormat::Hour24,
            seconds: SecondsMode::Hidden,
            min: None,
            max: None,
            step_minutes: 1,
            first_day_of_week: None,
            show_calendar_button: true,
            separator: " ".to_string(),
            placeholder: String::new(),
            enabled: true,
            read_only: false,
            label: None,
            validation_behavior: ValidationBehavior::AutoCorrect,
            feedback: Signal::new(ValidationFeedback::Pristine),
            on_value_changed: None,
            root_child_id: None,
        }
    }

    pub fn required(value: Signal<DateTime>) -> Self {
        let proxy: Signal<Option<DateTime>> = Signal::new(Some(value.get()));
        let mut s = Self::new(proxy);
        s.required_source = Some(value);
        s
    }

    pub fn date_format_pattern(mut self, p: impl Into<String>) -> Self {
        self.date_format_pattern = Some(p.into());
        self
    }

    pub fn time_format(mut self, f: TimeFormat) -> Self {
        self.time_format = f;
        self
    }

    pub fn seconds(mut self, mode: SecondsMode) -> Self {
        self.seconds = mode;
        self
    }

    pub fn min(mut self, dt: DateTime) -> Self {
        self.min = Some(dt);
        self
    }

    pub fn max(mut self, dt: DateTime) -> Self {
        self.max = Some(dt);
        self
    }

    pub fn step_minutes(mut self, n: u32) -> Self {
        self.step_minutes = n.max(1);
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

    pub fn separator(mut self, s: impl Into<String>) -> Self {
        self.separator = s.into();
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

    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }

    /// How parse failures are surfaced. Forwarded to both halves —
    /// each child editor uses the same behaviour.
    pub fn validation_behavior(mut self, behavior: ValidationBehavior) -> Self {
        self.validation_behavior = behavior;
        self
    }

    /// Reactive handle on the composed validation feedback. Reflects
    /// whichever half is more severe (Invalid > Corrected > Valid >
    /// Pristine). Each half still renders its own `ValidationStrip`
    /// — this is for apps that want to observe the overall outcome.
    pub fn validation_feedback_signal(&self) -> Signal<ValidationFeedback> {
        self.feedback.clone()
    }

    pub fn on_value_changed(
        mut self,
        f: impl Fn(Option<DateTime>, &mut EventContext) + 'static,
    ) -> Self {
        self.on_value_changed = Some(Rc::new(f));
        self
    }

    pub fn value(&self) -> Signal<Option<DateTime>> {
        self.value.clone()
    }
}

impl Widget for DateTimeEdit {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // ── required-source mirror via ctx.effect ─────────────
        // Two-way mirror between the optional outer signal (always
        // initialized to `Some(_)`) and the required-mode source.
        // observers from `signal.observe(...)` would be dropped at
        // construction time; effects live with the widget.
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
                    if let Some(dt) = v {
                        if src_clone.get() != *dt {
                            src_clone.set(*dt);
                        }
                    }
                });
            }
        }

        // ── outer → halves mirror ─────────────────────────────
        // Pure data sync, no callback fires (the user's
        // `on_value_changed` is reserved for events the user caused).
        {
            let date_part = self.date_part.clone();
            let time_part = self.time_part.clone();
            ctx.effect(&self.value, move |new_dt| {
                let new_d = new_dt.map(|dt| dt.date());
                let new_t = new_dt.map(|dt| dt.time());
                if date_part.get() != new_d {
                    date_part.set(new_d);
                }
                if time_part.get() != new_t {
                    time_part.set(new_t);
                }
            });
        }

        // ── Build the two child editors. ──────────────────────
        // The child editors' `on_value_changed` fires from a real
        // user event with an `EventContext`, which we use both to
        // recompose into the outer `value` signal AND to fire our
        // own `on_value_changed` callback. This is the chain that
        // lets DateTimeEdit consumers receive an EventContext for
        // every committed mutation — observers/effects can't, since
        // they fire outside of any event dispatch.
        let mut date_editor = DateEdit::new(self.date_part.clone());
        if let Some(min) = self.min {
            date_editor = date_editor.min_date(min.date());
        }
        if let Some(max) = self.max {
            date_editor = date_editor.max_date(max.date());
        }
        if let Some(p) = self.date_format_pattern.clone() {
            date_editor = date_editor.format_pattern(p);
        }
        if let Some(fdow) = self.first_day_of_week {
            date_editor = date_editor.first_day_of_week(fdow);
        }
        date_editor = date_editor
            .show_calendar_button(self.show_calendar_button)
            .calendar_popover_placement(OverlayPlacement::BelowPreferred)
            .enabled(self.enabled)
            .read_only(self.read_only)
            .validation_behavior(self.validation_behavior);
        let date_feedback = date_editor.validation_feedback_signal();
        {
            let outer = self.value.clone();
            let time_part = self.time_part.clone();
            let on_changed = self.on_value_changed.clone();
            date_editor = date_editor.on_value_changed(move |new_d, ctx_evt| {
                let combined = match (new_d, time_part.get()) {
                    (Some(d), Some(t)) => Some(d.to_datetime(t)),
                    _ => None,
                };
                if outer.get() != combined {
                    outer.set(combined);
                    if let Some(cb) = on_changed.as_ref() {
                        cb(combined, ctx_evt);
                    }
                }
            });
        }
        let date_id = ctx.add(date_editor);

        let mut time_editor = TimeEdit::new(self.time_part.clone())
            .format(self.time_format)
            .seconds(self.seconds)
            .step_minutes(self.step_minutes)
            .enabled(self.enabled)
            .read_only(self.read_only)
            .validation_behavior(self.validation_behavior);
        let time_feedback = time_editor.validation_feedback_signal();
        {
            let outer = self.value.clone();
            let date_part = self.date_part.clone();
            let on_changed = self.on_value_changed.clone();
            time_editor = time_editor.on_value_changed(move |new_t, ctx_evt| {
                let combined = match (date_part.get(), new_t) {
                    (Some(d), Some(t)) => Some(d.to_datetime(t)),
                    _ => None,
                };
                if outer.get() != combined {
                    outer.set(combined);
                    if let Some(cb) = on_changed.as_ref() {
                        cb(combined, ctx_evt);
                    }
                }
            });
        }
        let time_id = ctx.add(time_editor);

        let separator = TextWidget::new_literal(self.separator.clone())
            .style(fern_tokens::TextStyleRole::Body)
            .single_line()
            .a11y_hidden();
        let separator_id = ctx.add(separator);

        let row = HStack::new()
            .spacing(4.0)
            .add_child(date_id)
            .add_child(separator_id)
            .add_child(time_id);
        let row_id = ctx.add(row);
        self.root_child_id = Some(row_id);

        // Bind value at AccessibilityOnly so the wrapper's
        // `set_value` (composed ISO datetime) refreshes when
        // either half mutates.
        let self_id = ctx.self_id();
        self.value.bind_to(
            self_id,
            ctx.binding_registry(),
            fern_core::binding::BindingLevel::AccessibilityOnly,
        );

        // ── compose validation feedback from both halves ──────
        // Severity ordering: Invalid > Corrected > Valid > Pristine.
        // When either half updates we recompute the worst-of-two and
        // publish it on `self.feedback`. Bound at AccessibilityOnly
        // so the wrapper's `Invalid::True` flag refreshes too.
        {
            let composed = self.feedback.clone();
            let other = time_feedback.clone();
            ctx.effect(&date_feedback, move |new_date| {
                let combined = compose_feedback(new_date, &other.get());
                if composed.get() != combined {
                    composed.set(combined);
                }
            });
        }
        {
            let composed = self.feedback.clone();
            let other = date_feedback.clone();
            ctx.effect(&time_feedback, move |new_time| {
                let combined = compose_feedback(&other.get(), new_time);
                if composed.get() != combined {
                    composed.set(combined);
                }
            });
        }
        self.feedback.bind_to(
            self_id,
            ctx.binding_registry(),
            fern_core::binding::BindingLevel::AccessibilityOnly,
        );

        vec![row_id]
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
        builder.set_role(Role::DateTimeInput);
        if let Some(ref label) = self.label {
            builder.set_name(label);
        } else {
            builder.set_name(resolve_message_widget("date-time-edit-name", &[]));
        }
        match self.value.get() {
            Some(dt) => {
                builder.set_value(format!(
                    "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                    dt.date().year(),
                    dt.date().month(),
                    dt.date().day(),
                    dt.time().hour(),
                    dt.time().minute(),
                    dt.time().second(),
                ));
            }
            None => {
                if !self.placeholder.is_empty() {
                    builder.set_placeholder(self.placeholder.clone());
                } else {
                    builder
                        .set_placeholder(resolve_message_widget("date-time-edit-placeholder", &[]));
                }
            }
        }
        if !self.enabled {
            builder.set_disabled();
        }
        if self.read_only {
            builder.set_read_only();
        }
        if matches!(self.feedback.get(), ValidationFeedback::Invalid { .. }) {
            // No typed setter on AccessNodeBuilder — go through the
            // inner accesskit::NodeBuilder. Same escape hatch the date /
            // time halves use.
            builder.inner_mut().set_invalid(fern_core::accesskit::Invalid::True);
        }
        builder.add_action(Action::Focus);
    }
}

/// Pick the more severe of two halves. Severity:
/// `Invalid` > `Corrected` > `Valid` > `Pristine`.
fn compose_feedback(
    a: &ValidationFeedback,
    b: &ValidationFeedback,
) -> ValidationFeedback {
    fn rank(fb: &ValidationFeedback) -> u8 {
        match fb {
            ValidationFeedback::Invalid { .. } => 3,
            ValidationFeedback::Corrected { .. } => 2,
            ValidationFeedback::Valid => 1,
            ValidationFeedback::Pristine => 0,
        }
    }
    if rank(a) >= rank(b) { a.clone() } else { b.clone() }
}
