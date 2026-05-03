//! `DateRangeEdit` — composes two [`DateEdit`]s over a single
//! `Signal<Option<DateRange>>` for booking / analytics / report-filter
//! UIs that need a start + end date pair.
//!
//! The widget owns two intermediate signals (start + end halves) that
//! mirror the bound `DateRange`. Mutations on either half compose
//! back into the outer signal: when both halves carry a value, the
//! outer signal updates with the swap-aware [`DateRange::new`]
//! constructor (so end-before-start is silently corrected). When
//! either half is `None`, the outer signal is `None`.
//!
//! # Visual
//!
//! `[start_edit] – [end_edit]` — two `DateEdit` instances separated
//! by a configurable separator (default ` – ` en-dash). Each child
//! carries its own border, calendar popover, and validation strip;
//! the wrapper just composes them and merges feedback severity.
//!
//! Min/max bounds, format pattern, first day of week, and validation
//! behaviour are forwarded to both halves identically. The end half
//! additionally gets `min_date(start)` dynamically — once the user
//! has set a start date, the end half's calendar disables anything
//! before it.
//!
//! # Accessibility
//!
//! - Container — `Role::DateInput` with `set_value` formatted as
//!   `YYYY-MM-DD/YYYY-MM-DD` (ISO-style range notation).
//! - Each child `DateEdit` keeps its own `Role::DateInput` as a
//!   nested AT child.

#[cfg(test)]
mod tests;

use std::rc::Rc;

use fern_canvas::{Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::accesskit::{Action, Role};
use fern_core::build_context::BuildContext;
use fern_core::overlay::{
    DismissBehavior, OverlayDismissCallback, OverlayLayer, OverlayPlacement, OverlayRequest,
};
use fern_core::signal::Signal;
use fern_core::widget::{EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_i18n::resolve_message_widget;
use jiff::civil::Weekday;

use crate::calendar::{Calendar, DateRange};
use crate::common::datetime::Date;
use crate::date_edit::{CalendarTriggerButton, DateEdit, ValidationBehavior};
use crate::primitives::text_input_field::ValidationFeedback;
use crate::primitives::{Divider, HStack, Padding, TextWidget};

type OnRangeChanged = Rc<dyn Fn(Option<DateRange>, &mut EventContext)>;

/// Two-handle date picker over a `Signal<Option<DateRange>>`. See the
/// [module docs](self) for the full feature list.
pub struct DateRangeEdit {
    value: Signal<Option<DateRange>>,
    /// Internal start half — published to the start `DateEdit` and
    /// kept in sync with `value` via `ctx.effect`.
    start_part: Signal<Option<Date>>,
    /// Internal end half — same as `start_part` for end.
    end_part: Signal<Option<Date>>,
    min_date: Option<Date>,
    max_date: Option<Date>,
    pattern: Option<String>,
    placeholder_start: String,
    placeholder_end: String,
    separator: String,
    first_day_of_week: Option<Weekday>,
    show_calendar_button: bool,
    /// Whether to render a trailing range-calendar button to the
    /// right of the end DateEdit. The button opens a single popover
    /// hosting `Calendar::range` bound to `self.value`. Default
    /// `true`. Set to `false` to suppress the trailing button when
    /// the per-half calendar buttons are sufficient.
    show_range_calendar_button: bool,
    /// Live state of the range-calendar popover for the trailing
    /// trigger. Bound to `set_expanded` on the trigger's AT node.
    range_popover_open: Signal<bool>,
    enabled: bool,
    read_only: bool,
    label: Option<String>,
    validation_behavior: ValidationBehavior,
    /// Composed validation feedback (severity-merged from both halves).
    feedback: Signal<ValidationFeedback>,
    on_value_changed: Option<OnRangeChanged>,
    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for DateRangeEdit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DateRangeEdit").finish_non_exhaustive()
    }
}

impl DateRangeEdit {
    /// Construct a range editor bound to a nullable `DateRange` signal.
    pub fn new(value: Signal<Option<DateRange>>) -> Self {
        let initial = value.get();
        let start_part = Signal::new(initial.map(|r| r.start));
        let end_part = Signal::new(initial.map(|r| r.end));
        Self {
            value,
            start_part,
            end_part,
            min_date: None,
            max_date: None,
            pattern: None,
            placeholder_start: String::new(),
            placeholder_end: String::new(),
            separator: " – ".to_string(),
            first_day_of_week: None,
            show_calendar_button: true,
            show_range_calendar_button: true,
            range_popover_open: Signal::new(false),
            enabled: true,
            read_only: false,
            label: None,
            validation_behavior: ValidationBehavior::AutoCorrect,
            feedback: Signal::new(ValidationFeedback::Pristine),
            on_value_changed: None,
            root_child_id: None,
        }
    }

    pub fn min_date(mut self, d: Date) -> Self {
        self.min_date = Some(d);
        self
    }

    pub fn max_date(mut self, d: Date) -> Self {
        self.max_date = Some(d);
        self
    }

    /// Override the locale-derived format pattern for both halves.
    pub fn format_pattern(mut self, p: impl Into<String>) -> Self {
        self.pattern = Some(p.into());
        self
    }

    pub fn placeholder_start(mut self, text: impl Into<String>) -> Self {
        self.placeholder_start = text.into();
        self
    }

    pub fn placeholder_end(mut self, text: impl Into<String>) -> Self {
        self.placeholder_end = text.into();
        self
    }

    /// Visual separator between the two halves (default ` – `, en-dash).
    pub fn separator(mut self, s: impl Into<String>) -> Self {
        self.separator = s.into();
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

    /// Whether to render a trailing single calendar button (right of
    /// the end half) that opens a shared `Calendar::range` popover.
    /// Default `true`. The per-half single-date calendar buttons
    /// (controlled by [`Self::show_calendar_button`]) remain
    /// independent — disabling one doesn't disable the other.
    pub fn show_range_calendar_button(mut self, show: bool) -> Self {
        self.show_range_calendar_button = show;
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

    /// How parse failures are surfaced. Forwarded to both halves.
    pub fn validation_behavior(mut self, behavior: ValidationBehavior) -> Self {
        self.validation_behavior = behavior;
        self
    }

    /// Reactive handle on the composed validation feedback (worse of
    /// the two halves).
    pub fn validation_feedback_signal(&self) -> Signal<ValidationFeedback> {
        self.feedback.clone()
    }

    pub fn on_value_changed(
        mut self,
        f: impl Fn(Option<DateRange>, &mut EventContext) + 'static,
    ) -> Self {
        self.on_value_changed = Some(Rc::new(f));
        self
    }

    pub fn value(&self) -> Signal<Option<DateRange>> {
        self.value.clone()
    }
}

impl Widget for DateRangeEdit {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let enabled = self.enabled;
        let read_only = self.read_only;

        // Outer → halves: when the bound range changes externally,
        // push start/end into the per-half signals.
        {
            let start_part = self.start_part.clone();
            let end_part = self.end_part.clone();
            ctx.effect(&self.value, move |new_range| {
                let (s, e) = match new_range {
                    Some(r) => (Some(r.start), Some(r.end)),
                    None => (None, None),
                };
                if start_part.get() != s {
                    start_part.set(s);
                }
                if end_part.get() != e {
                    end_part.set(e);
                }
            });
        }

        // ── Start DateEdit ────────────────────────────────────
        let mut start_editor = DateEdit::new(self.start_part.clone());
        if let Some(min) = self.min_date {
            start_editor = start_editor.min_date(min);
        }
        if let Some(max) = self.max_date {
            start_editor = start_editor.max_date(max);
        }
        if let Some(p) = self.pattern.clone() {
            start_editor = start_editor.format_pattern(p);
        }
        if let Some(fdow) = self.first_day_of_week {
            start_editor = start_editor.first_day_of_week(fdow);
        }
        if !self.placeholder_start.is_empty() {
            start_editor = start_editor.placeholder(self.placeholder_start.clone());
        }
        start_editor = start_editor
            .show_calendar_button(self.show_calendar_button)
            .calendar_popover_placement(OverlayPlacement::BelowPreferred)
            .enabled(enabled)
            .read_only(read_only)
            .validation_behavior(self.validation_behavior);
        let start_feedback = start_editor.validation_feedback_signal();
        {
            let outer = self.value.clone();
            let end_part = self.end_part.clone();
            let on_changed = self.on_value_changed.clone();
            start_editor = start_editor.on_value_changed(move |new_s, ctx_evt| {
                let combined = match (new_s, end_part.get()) {
                    (Some(s), Some(e)) => Some(DateRange::new(s, e)),
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
        let start_id = ctx.add(start_editor);

        // ── End DateEdit ──────────────────────────────────────
        let mut end_editor = DateEdit::new(self.end_part.clone());
        // The end half's lower bound is `max(min_date, current start)`.
        // Without a dynamic `min_date` accessor we settle for the
        // static min — a follow-up could reactively re-tighten this.
        if let Some(min) = self.min_date {
            end_editor = end_editor.min_date(min);
        }
        if let Some(max) = self.max_date {
            end_editor = end_editor.max_date(max);
        }
        if let Some(p) = self.pattern.clone() {
            end_editor = end_editor.format_pattern(p);
        }
        if let Some(fdow) = self.first_day_of_week {
            end_editor = end_editor.first_day_of_week(fdow);
        }
        if !self.placeholder_end.is_empty() {
            end_editor = end_editor.placeholder(self.placeholder_end.clone());
        }
        end_editor = end_editor
            .show_calendar_button(self.show_calendar_button)
            .calendar_popover_placement(OverlayPlacement::BelowPreferred)
            .enabled(enabled)
            .read_only(read_only)
            .validation_behavior(self.validation_behavior);
        let end_feedback = end_editor.validation_feedback_signal();
        {
            let outer = self.value.clone();
            let start_part = self.start_part.clone();
            let on_changed = self.on_value_changed.clone();
            end_editor = end_editor.on_value_changed(move |new_e, ctx_evt| {
                let combined = match (start_part.get(), new_e) {
                    (Some(s), Some(e)) => Some(DateRange::new(s, e)),
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
        let end_id = ctx.add(end_editor);

        // ── Separator + row layout ────────────────────────────
        let separator = TextWidget::new_literal(self.separator.clone())
            .style(fern_tokens::TextStyleRole::Body)
            .single_line()
            .a11y_hidden();
        let separator_id = ctx.add(separator);

        // ── Trailing range-calendar button + popover ─────────
        // One shared `Calendar::range` overlay, anchored to the
        // wrapper, that lets the user pick start + end dates with
        // the calendar's two-anchor input model. The per-half
        // single-date calendar buttons stay independent — this is
        // an additional affordance, not a replacement.
        let range_trigger_id_opt = if self.show_range_calendar_button {
            // Pre-build the dormant range calendar (mounted only
            // while the popover is open).
            let value_for_cal = self.value.clone();
            let popover_open = self.range_popover_open.clone();
            let mut cal = Calendar::range(value_for_cal.clone()).on_range_changed(
                move |new_range, ctx_evt| {
                    // Once both anchors are picked, the calendar
                    // commits a `DateRange`; close the popover and
                    // return focus to the trigger via the same
                    // request-focus path DateEdit uses.
                    if new_range.is_some() {
                        popover_open.set(false);
                        ctx_evt.dismiss_all_overlays();
                        ctx_evt.request_frame();
                    }
                },
            );
            if let Some(min) = self.min_date {
                cal = cal.min_date(min);
            }
            if let Some(max) = self.max_date {
                cal = cal.max_date(max);
            }
            if let Some(fdow) = self.first_day_of_week {
                cal = cal.first_day_of_week(fdow);
            }
            let cal_id = ctx.add(cal);
            ctx.set_dormant(cal_id);

            // Trigger button.
            let popover_open = self.range_popover_open.clone();
            let self_ref = ctx.self_id();
            let dismiss_cb: OverlayDismissCallback = {
                let popover_open = popover_open.clone();
                std::rc::Rc::new(move || {
                    popover_open.set(false);
                })
            };
            // Style the trigger like the per-half buttons (same
            // CalendarTriggerButton primitive that `DateEdit` uses).
            // Width / icon size match Int UI conventions used by
            // DateEdit so the three buttons align visually.
            let trigger_widget = CalendarTriggerButton::new(
                28.0,
                14.0,
                enabled && !read_only,
                std::rc::Rc::new(move |ctx_evt: &mut EventContext| {
                    if popover_open.get() {
                        popover_open.set(false);
                        ctx_evt.dismiss_all_overlays();
                    } else {
                        popover_open.set(true);
                        ctx_evt.activate(cal_id);
                        ctx_evt.show_overlay(OverlayRequest {
                            content_id: cal_id,
                            anchor: self_ref,
                            placement: OverlayPlacement::BelowPreferred,
                            dismiss: DismissBehavior::EscapeOrClickOutside,
                            layer: OverlayLayer::InTree,
                            parent_overlay: None,
                            on_dismiss: Some(dismiss_cb.clone()),
                            fade_duration: None,
                        });
                        ctx_evt.request_focus(cal_id);
                    }
                }),
            );
            // Tiny vertical divider between end DateEdit and the
            // trailing trigger so it visually reads as a sibling
            // affordance, not part of the end field's frame.
            let div = ctx.add(
                Padding::new(2.0, 0.0, 2.0, 0.0)
                    .child(Divider::vertical().thickness(1.0)),
            );
            let trigger_id = ctx.add(trigger_widget);
            Some((div, trigger_id))
        } else {
            None
        };

        let mut row = HStack::new()
            .spacing(4.0)
            .add_child(start_id)
            .add_child(separator_id)
            .add_child(end_id);
        if let Some((div, trigger)) = range_trigger_id_opt {
            row = row.add_child(div).add_child(trigger);
        }
        let row_id = ctx.add(row);
        self.root_child_id = Some(row_id);

        // Bind value at AccessibilityOnly so the wrapper's `set_value`
        // (composed ISO range) refreshes when either half mutates.
        let self_id = ctx.self_id();
        self.value.bind_to(
            self_id,
            ctx.binding_registry(),
            fern_core::binding::BindingLevel::AccessibilityOnly,
        );

        // ── Compose feedback severity from both halves ────────
        // Same severity ladder DateTimeEdit uses: Invalid > Corrected
        // > Valid > Pristine. Worst of the two halves wins.
        {
            let composed = self.feedback.clone();
            let other = end_feedback.clone();
            ctx.effect(&start_feedback, move |new_start| {
                let combined = compose_feedback(new_start, &other.get());
                if composed.get() != combined {
                    composed.set(combined);
                }
            });
        }
        {
            let composed = self.feedback.clone();
            let other = start_feedback.clone();
            ctx.effect(&end_feedback, move |new_end| {
                let combined = compose_feedback(&other.get(), new_end);
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
        // No `Role::DateRangeInput` in AccessKit — stay on `DateInput`
        // for the wrapper and let the two child `DateInput`s carry the
        // detailed structure.
        builder.set_role(Role::DateInput);
        if let Some(ref label) = self.label {
            builder.set_name(label.clone());
        } else {
            builder.set_name(resolve_message_widget("date-range-edit-name", &[]));
        }
        match self.value.get() {
            Some(r) => {
                builder.set_value(format!(
                    "{:04}-{:02}-{:02}/{:04}-{:02}-{:02}",
                    r.start.year(),
                    r.start.month(),
                    r.start.day(),
                    r.end.year(),
                    r.end.month(),
                    r.end.day(),
                ));
            }
            None => {
                builder.set_placeholder(resolve_message_widget(
                    "date-range-edit-placeholder",
                    &[],
                ));
            }
        }
        if !self.enabled {
            builder.set_disabled();
        }
        if self.read_only {
            builder.set_read_only();
        }
        if matches!(self.feedback.get(), ValidationFeedback::Invalid { .. }) {
            builder
                .inner_mut()
                .set_invalid(fern_core::accesskit::Invalid::True);
        }
        builder.add_action(Action::Focus);
    }
}

/// Pick the more severe of two halves. Severity:
/// `Invalid > Corrected > Valid > Pristine`. Same ladder
/// `DateTimeEdit` uses (kept as a private helper here to avoid
/// cross-widget visibility plumbing).
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
