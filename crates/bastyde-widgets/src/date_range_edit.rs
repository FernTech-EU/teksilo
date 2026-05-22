//! `DateRangeEdit` — single unified control for picking a `DateRange`.
//!
//! Visually one widget: a single bordered frame containing two
//! `TextInputField` halves separated by a painted arrow glyph, with
//! a trailing built-in calendar button that opens a shared
//! `Calendar::range` popover. Backed by `Signal<Option<DateRange>>`.
//!
//! ```text
//! ┌──────────────────────────────────────┐
//! │ 05/12/2026   →   05/19/2026   │ 📅  │
//! └──────────────────────────────────────┘
//! ```
//!
//! # Why one frame?
//!
//! Two adjacent `DateEdit`s (one frame each) visually read as two
//! separate fields that happen to be next to each other. A single
//! frame says "this is one range". Same affordance the user is used
//! to from booking sites and analytics dashboards.
//!
//! # Behaviour
//!
//! - **Two text halves** — each masked from the resolved date pattern,
//!   each with its own validator + segment-stepping (Up/Down on the
//!   focused segment matches `DateEdit`).
//! - **Painted arrow separator** — a thin chevron-right glyph, no text.
//!   Visual only; AT users see the wrapper's `Role::DateInput`.
//! - **One trailing calendar button** — Int UI `IconButton::embedded()` with
//!   the calendar glyph. Opens a single popover hosting
//!   `Calendar::range` bound to the outer signal. The two-anchor
//!   click model (start-then-end) commits the range and closes the
//!   popover. No per-half calendar buttons — there's only one
//!   calendar, anchored to the wrapper.
//! - **One frame** — focus-aware border (`BorderRole::Focused` while
//!   any half holds focus, otherwise `Default`), validation-aware
//!   border (`Error` for `Invalid`, `Focused` for `Corrected`).
//! - **One validation strip** below the frame — composed feedback
//!   from both halves (worse of the two wins).
//!
//! # Accessibility
//!
//! - Container — `Role::DateInput` with `set_value` formatted as
//!   `YYYY-MM-DD/YYYY-MM-DD` (ISO range).
//! - Each `TextInputField` keeps its own `Role::TextInput` AT node;
//!   the wrapper's `Role::DateInput` provides the range semantics.

#[cfg(test)]
mod tests;

use bastyde_i18n::localized;
use std::rc::Rc;

use bastyde_canvas::{Path, Point, Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::accesskit::{Action, Role};
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::overlay::{
    DismissBehavior, OverlayDismissCallback, OverlayLayer, OverlayPlacement, OverlayRequest,
};
use bastyde_core::signal::Signal;
use bastyde_core::widget::{EventContext, LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::{HandlerSet, WidgetBuilder};
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::resolve_message_widget;
use bastyde_tokens::{BorderRole, CornerRadius, SurfaceRole};
use jiff::civil::Weekday;

use crate::calendar::{Calendar, DateRange};
use crate::common::datetime::Date;
use crate::common::datetime::pattern::{
    ParseTarget, ParsedPattern, ParsedValue, format_value, mask_for_pattern, parse_value,
    segment_at_position, step_date_field,
};
use crate::common::datetime::types::today_local;
use crate::date_edit::{ValidationBehavior, build_date_validator, calendar_glyph_icon, clamp_date};
use crate::icon_button::{IconButton, IconButtonSize};
use crate::primitives::text_input_field::{TextInputField, ValidationFeedback};
use crate::primitives::{
    Center, FixedSize, HStack, IconWidget, MinSize, Padding, RectWidget, VStack, ZStack,
};

type OnRangeChanged = Rc<dyn Fn(Option<DateRange>, &mut EventContext)>;

/// Two-handle date picker over `Signal<Option<DateRange>>`. See the
/// [module docs](self) for the visual layout and behaviour.
pub struct DateRangeEdit {
    value: Signal<Option<DateRange>>,
    /// Internal start half — drives the start `TextInputField` text
    /// signal and is kept in sync with `value` via `ctx.effect`.
    start_part: Signal<Option<Date>>,
    end_part: Signal<Option<Date>>,
    start_text: Signal<String>,
    end_text: Signal<String>,
    min_date: Option<Date>,
    max_date: Option<Date>,
    pattern: Option<String>,
    placeholder_start: String,
    placeholder_end: String,
    first_day_of_week: Option<Weekday>,
    /// Initial enabled-state; forwarded to the arena at build time.
    initial_enabled: bool,
    read_only: bool,
    label: Option<String>,
    validation_behavior: ValidationBehavior,
    /// How the trailing (end) half claims horizontal space. The
    /// leading (start) half always sizes to its mask-derived
    /// natural width — the start date stays put while the end half
    /// either matches that natural width
    /// ([`WidthPolicy::Default`]) or absorbs whatever extra space
    /// the parent offers ([`WidthPolicy::Fill`]).
    end_width_policy: crate::date_edit::WidthPolicy,
    /// Composed validation feedback (severity-merged from both halves).
    feedback: Signal<ValidationFeedback>,
    /// `true` while either half holds keyboard focus — drives the
    /// unified frame border.
    focused: Signal<bool>,
    /// `true` while the calendar popover is open — drives the
    /// trigger's AT `set_expanded` and the open/close toggle.
    range_popover_open: Signal<bool>,
    on_value_changed: Option<OnRangeChanged>,
    style_override: Option<bastyde_core::styles::SharedDateEditStyle>,
    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for DateRangeEdit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DateRangeEdit").finish_non_exhaustive()
    }
}

impl DateRangeEdit {
    pub fn new(value: Signal<Option<DateRange>>) -> Self {
        let initial = value.get();
        let start_part = Signal::new(initial.map(|r| r.start));
        let end_part = Signal::new(initial.map(|r| r.end));
        Self {
            value,
            start_part,
            end_part,
            start_text: Signal::new(String::new()),
            end_text: Signal::new(String::new()),
            min_date: None,
            max_date: None,
            pattern: None,
            placeholder_start: String::new(),
            placeholder_end: String::new(),
            first_day_of_week: None,
            initial_enabled: true,
            read_only: false,
            label: None,
            validation_behavior: ValidationBehavior::AutoCorrect,
            end_width_policy: crate::date_edit::WidthPolicy::Default,
            feedback: Signal::new(ValidationFeedback::Pristine),
            focused: Signal::new(false),
            range_popover_open: Signal::new(false),
            on_value_changed: None,
            style_override: None,
            root_child_id: None,
        }
    }

    /// Per-call DateEditStyle override (shared with DateEdit family).
    pub fn style(mut self, style: impl bastyde_core::styles::DateEditStyle) -> Self {
        self.style_override = Some(std::rc::Rc::new(style));
        self
    }

    pub fn min_date(mut self, d: Date) -> Self {
        self.min_date = Some(d);
        self
    }

    pub fn max_date(mut self, d: Date) -> Self {
        self.max_date = Some(d);
        self
    }

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

    pub fn first_day_of_week(mut self, w: Weekday) -> Self {
        self.first_day_of_week = Some(w);
        self
    }

    /// Set the initial enabled state. Forwarded to the arena at build time.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.initial_enabled = enabled;
        self
    }

    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub fn label(mut self, label: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        let ls: bastyde_i18n::LocalizedString = label.into();
        self.label = Some(ls.resolve_now());
        self
    }

    pub fn validation_behavior(mut self, behavior: ValidationBehavior) -> Self {
        self.validation_behavior = behavior;
        self
    }

    /// How the trailing (end) half claims horizontal space. The
    /// leading (start) half always sizes to its natural mask width;
    /// the end half follows this policy. Default
    /// [`WidthPolicy::Default`] (natural width); pass
    /// [`WidthPolicy::Fill`] to make the end half absorb extra
    /// space the parent offers.
    pub fn end_width_policy(mut self, policy: crate::date_edit::WidthPolicy) -> Self {
        self.end_width_policy = policy;
        self
    }

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
        let theme = ctx.theme_signal().get();
        use crate::styles::recipe_date_edit_style as de;
        use crate::styles::recipe_text_input_style as field_dims;
        let focus_ring_width = theme.shape.focus_ring_width;
        let self_id = ctx.self_id();
        // Forward initial-enabled into the arena; see IconButton.
        if !self.initial_enabled {
            ctx.enabled_when(self_id, false);
        }
        let enabled = self.initial_enabled;
        let read_only = self.read_only;

        // Resolve pattern — locale default unless overridden.
        let pattern_string = self.pattern.clone().unwrap_or_else(|| {
            let tag = ctx.locale_signal().get().unwrap_or_default();
            crate::common::datetime::format_pattern_for_locale(&tag).to_string()
        });
        let parsed_pattern = ParsedPattern::parse(&pattern_string)
            .unwrap_or_else(|_| ParsedPattern::parse("%Y-%m-%d").unwrap());
        let pattern_rc = Rc::new(parsed_pattern);
        let mask_string = mask_for_pattern(&pattern_rc);
        let min = self.min_date;
        let max = self.max_date;

        // Outer → halves: when the bound range changes externally,
        // push start/end into the per-half date signals AND reformat
        // their text. The text reformat is necessary so a programmatic
        // `value.set(...)` shows up in the visible field, not just the
        // hidden state.
        {
            let start_part = self.start_part.clone();
            let end_part = self.end_part.clone();
            let start_text = self.start_text.clone();
            let end_text = self.end_text.clone();
            let pattern = pattern_rc.clone();
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
                let s_text = s
                    .map(|d| format_value(&pattern, Some(d), None))
                    .unwrap_or_default();
                let e_text = e
                    .map(|d| format_value(&pattern, Some(d), None))
                    .unwrap_or_default();
                if start_text.get() != s_text {
                    start_text.set(s_text);
                }
                if end_text.get() != e_text {
                    end_text.set(e_text);
                }
            });
        }
        // Seed text once at build time so the field shows the initial
        // value without waiting for the first effect tick.
        {
            self.start_text.set(
                self.start_part
                    .get()
                    .map(|d| format_value(&pattern_rc, Some(d), None))
                    .unwrap_or_default(),
            );
            self.end_text.set(
                self.end_part
                    .get()
                    .map(|d| format_value(&pattern_rc, Some(d), None))
                    .unwrap_or_default(),
            );
        }

        // ── Build each half as a bare TextInputField ───────────
        let start_field_id = self.build_half(
            ctx,
            HalfKind::Start,
            pattern_rc.clone(),
            &mask_string,
            min,
            max,
        );
        let end_field_id = self.build_half(
            ctx,
            HalfKind::End,
            pattern_rc.clone(),
            &mask_string,
            min,
            max,
        );

        // ── Painted arrow separator ────────────────────────────
        let separator_icon = arrow_right_icon(field_dims::TEXT_FIELD_HEIGHT * 0.45)
            .bind_color(bastyde_tokens::TextRole::Secondary);
        let separator_id = ctx.add(
            FixedSize::new()
                .bind_width(field_dims::TEXT_FIELD_HEIGHT * 0.65)
                .bind_height(field_dims::TEXT_FIELD_HEIGHT)
                .child(Center::new().child(separator_icon)),
        );

        // ── Trailing calendar trigger ──────────────────────────
        // Pre-build the dormant range calendar once.
        let value_for_cal = self.value.clone();
        let popover_open_for_cal = self.range_popover_open.clone();
        let mut cal =
            Calendar::range(value_for_cal.clone()).on_range_changed(move |new_range, ctx_evt| {
                if new_range.is_some() {
                    popover_open_for_cal.set(false);
                    ctx_evt.dismiss_self_overlay_chain();
                    ctx_evt.request_frame();
                }
            });
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

        let popover_open = self.range_popover_open.clone();
        let self_ref = ctx.self_id();
        let dismiss_cb: OverlayDismissCallback = {
            let popover_open = popover_open.clone();
            Rc::new(move || {
                popover_open.set(false);
            })
        };
        let trigger_btn = IconButton::new(calendar_glyph_icon(de::CALENDAR_ICON_SIZE))
            .embedded()
            .size(IconButtonSize::Default)
            .enabled(enabled && !read_only)
            .tooltip(localized(move || {
                resolve_message_widget("date-range-edit-trigger-tooltip", &[])
            }))
            .on_activate_fn(move |ctx_evt: &mut EventContext| {
                if popover_open.get() {
                    popover_open.set(false);
                    ctx_evt.dismiss_all_except_hosts();
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
            });
        let trigger_id = ctx.add(trigger_btn);

        // ── Row layout ─────────────────────────────────────────
        // No divider before the trailing trigger — Int UI's
        // embedded IconButton sits flush inside the field's trailing slot
        // (the same convention TextInput uses) and the button's own
        // hover/pressed background gives it enough visual separation.
        let row = HStack::new()
            .spacing(0.0)
            .add_child(start_field_id)
            .add_child(separator_id)
            .add_child(end_field_id)
            .add_child(trigger_id);
        let inline_row_id = ctx.add(row);
        let row_id = ctx.add(
            Padding::new(
                0.0,
                field_dims::TEXT_FIELD_PADDING_HORIZONTAL,
                0.0,
                field_dims::TEXT_FIELD_PADDING_HORIZONTAL,
            )
            .child_id(inline_row_id),
        );

        // ── Frame: bg + border driven by focus + validation ───
        let feedback_for_border = self.feedback.clone();
        let focused_for_border = self.focused.clone();
        let border_role =
            focused_for_border
                .clone()
                .zip(&feedback_for_border)
                .map(|(focused, fb)| match fb {
                    ValidationFeedback::Invalid { .. } => BorderRole::Error,
                    ValidationFeedback::Corrected { .. } if !*focused => BorderRole::Focused,
                    _ => {
                        if *focused {
                            BorderRole::Focused
                        } else {
                            BorderRole::Default
                        }
                    }
                });
        let border_width_signal =
            focused_for_border
                .clone()
                .zip(&feedback_for_border)
                .map(move |(focused, fb)| {
                    if *focused || matches!(fb, ValidationFeedback::Invalid { .. }) {
                        focus_ring_width
                    } else {
                        field_dims::TEXT_FIELD_BORDER_WIDTH
                    }
                });
        let bg = RectWidget::new()
            .background(SurfaceRole::Content)
            .border_color(border_role)
            .border_width(border_width_signal)
            .corner_radius(CornerRadius::uniform(field_dims::TEXT_FIELD_CORNER_RADIUS));
        let bg_id = ctx.add(bg);
        let framed_id = ctx.add(ZStack::new().add_child(bg_id).add_child(row_id));
        let sized_id =
            ctx.add(MinSize::new(0.0, field_dims::TEXT_FIELD_HEIGHT).child_id(framed_id));

        // ── Inline validation strip below the frame ───────────
        let strip_id = ctx.add(crate::primitives::ValidationStrip::new(
            self.feedback.clone(),
        ));
        let root_with_strip = ctx.add(
            VStack::new()
                .spacing(field_dims::TEXT_FIELD_VALIDATION_STRIP_GAP)
                .add_child(sized_id)
                .add_child(strip_id),
        );
        let style = crate::styles::recipe_date_edit_style::resolve_date_edit_style(
            &self.style_override,
            ctx,
        );
        let cfg = bastyde_core::styles::DateEditStyleConfig {
            body: root_with_strip,
        };
        let root_id = style.make_body(&cfg, ctx);
        self.root_child_id = Some(root_id);

        // ── Self handlers: focus_within drives the frame border ─
        let handlers = HandlerSet::new().focus_within(self.focused.clone());
        ctx.apply_self_handlers(handlers);

        // Bind the value at AccessibilityOnly so the wrapper's AT
        // node refreshes set_value when either half mutates.
        let self_id = ctx.self_id();
        self.value.bind_to(
            self_id,
            ctx.binding_registry(),
            bastyde_core::binding::BindingLevel::AccessibilityOnly,
        );
        self.feedback.bind_to(
            self_id,
            ctx.binding_registry(),
            bastyde_core::binding::BindingLevel::AccessibilityOnly,
        );
        self.range_popover_open.bind_to(
            self_id,
            ctx.binding_registry(),
            bastyde_core::binding::BindingLevel::AccessibilityOnly,
        );
        // Suppress unused-field warning until we surface the trigger
        // a11y separately.
        let _ = trigger_id;

        vec![root_with_strip]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        // Forward the inner LayoutResponse, then overlay flex=1 when
        // the end half is Fill — the inner HStack consumes its
        // children's flex internally and reports flex=0 to its
        // parents, so the outer wrapper has to advertise flex
        // explicitly for parent stacks to allocate slack.
        let response = match self.root_child_id {
            Some(id) => ctx
                .child_layout_response(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into()),
            None => proposal.resolve(0.0, 0.0).into(),
        };
        if self.end_width_policy == crate::date_edit::WidthPolicy::Fill {
            bastyde_core::widget::LayoutResponse::flexible(response.size, 1.0)
        } else {
            response
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
                builder.set_placeholder(resolve_message_widget("date-range-edit-placeholder", &[]));
            }
        }
        // Framework a11y walker sets `set_disabled` from arena state.
        if self.read_only {
            builder.set_read_only();
        }
        if matches!(self.feedback.get(), ValidationFeedback::Invalid { .. }) {
            builder
                .inner_mut()
                .set_invalid(bastyde_core::accesskit::Invalid::True);
        }
        builder.add_action(Action::Focus);
    }
}

#[derive(Clone, Copy)]
enum HalfKind {
    Start,
    End,
}

impl DateRangeEdit {
    /// Build one half (start or end) as a bare `TextInputField` with
    /// mask + validator + segment-stepping wired against the
    /// appropriate per-half text/date signals. Returns the WidgetId
    /// wrapped in a fixed-width container so both halves visually
    /// align inside the unified frame.
    #[allow(clippy::too_many_arguments)]
    fn build_half(
        &self,
        ctx: &mut BuildContext,
        kind: HalfKind,
        pattern_rc: Rc<ParsedPattern>,
        mask_string: &str,
        min: Option<Date>,
        max: Option<Date>,
    ) -> WidgetId {
        use crate::styles::recipe_text_input_style as field_dims;
        let (text_signal, date_signal, placeholder, other_date) = match kind {
            HalfKind::Start => (
                self.start_text.clone(),
                self.start_part.clone(),
                self.placeholder_start.clone(),
                self.end_part.clone(),
            ),
            HalfKind::End => (
                self.end_text.clone(),
                self.end_part.clone(),
                self.placeholder_end.clone(),
                self.start_part.clone(),
            ),
        };

        let validator =
            build_date_validator(pattern_rc.clone(), min, max, self.validation_behavior);

        let outer_value = self.value.clone();
        let on_changed = self.on_value_changed.clone();
        let merge_into_outer =
            move |new_d: Option<Date>, other_d: Option<Date>, ctx_evt: &mut EventContext| {
                let combined = match kind {
                    HalfKind::Start => match (new_d, other_d) {
                        (Some(s), Some(e)) => Some(DateRange::new(s, e)),
                        _ => None,
                    },
                    HalfKind::End => match (other_d, new_d) {
                        (Some(s), Some(e)) => Some(DateRange::new(s, e)),
                        _ => None,
                    },
                };
                if outer_value.get() != combined {
                    outer_value.set(combined);
                    if let Some(cb) = on_changed.as_ref() {
                        cb(combined, ctx_evt);
                    }
                }
            };

        // Commit closure: parse the field text on Enter / blur, sync
        // the per-half date signal, then merge into the outer range.
        let commit: Rc<dyn Fn(&mut EventContext)> = {
            let text_signal = text_signal.clone();
            let date_signal = date_signal.clone();
            let other_date = other_date.clone();
            let pattern = pattern_rc.clone();
            let merge = merge_into_outer.clone();
            Rc::new(move |ctx_evt: &mut EventContext| {
                let raw = text_signal.get();
                let trimmed = raw.trim();
                let parsed: Option<Date> = if trimmed.is_empty() {
                    None
                } else {
                    match parse_value(&pattern, trimmed, ParseTarget::DateOnly) {
                        Some(ParsedValue::Date(d)) => Some(clamp_date(d, min, max)),
                        _ => date_signal.get(),
                    }
                };
                if date_signal.get() != parsed {
                    date_signal.set(parsed);
                }
                merge(parsed, other_date.get(), ctx_evt);
            })
        };

        let inner_height =
            (field_dims::TEXT_FIELD_HEIGHT - 2.0 * field_dims::TEXT_FIELD_BORDER_WIDTH).max(0.0);
        let text_area_height =
            (inner_height - 2.0 * field_dims::TEXT_FIELD_PADDING_VERTICAL).max(0.0);

        let pattern_for_filter = pattern_rc.clone();
        let mut field = TextInputField::new(text_signal.clone())
            .enabled(self.initial_enabled)
            .read_only(self.read_only)
            .placeholder(placeholder)
            .text_height(text_area_height)
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
                    if let crate::common::datetime::pattern::PatternToken::Literal(s) = tok
                        && s.chars().any(|x| x == c)
                    {
                        return true;
                    }
                }
                false
            });
        // Mirror the inner field's feedback into the outer composed
        // feedback signal (worse-of-two semantics).
        {
            let inner_feedback = field.validation_feedback_signal();
            let composed = self.feedback.clone();
            let other_feedback_owner = match kind {
                HalfKind::Start => Some(self.feedback.clone()), // placeholder, replaced below
                HalfKind::End => Some(self.feedback.clone()),
            };
            // The other half's feedback isn't accessible at this point
            // (it's inside its own field). Compose via a per-half
            // mirror: each half's effect computes max(self, current
            // composed) → composed. As long as both halves install
            // this, the worse always wins.
            let _ = other_feedback_owner;
            ctx.effect(&inner_feedback, move |new_fb| {
                let merged = match (composed.get(), new_fb.clone()) {
                    (a, b) if rank(&a) >= rank(&b) => a,
                    (_, b) => b,
                };
                if composed.get() != merged {
                    composed.set(merged);
                }
            });
        }
        {
            let commit = commit.clone();
            field = field.on_submit_fn(move |ctx_evt| commit(ctx_evt));
        }
        {
            let commit = commit.clone();
            field = field.on_blur_fn(move |ctx_evt| commit(ctx_evt));
        }

        // Capture caret signal + caret_setter BEFORE moving the field
        // into the tree, for segment-stepping.
        let caret = field.caret_position();
        let caret_setter = field.caret_setter();

        // A11y: TimeInput-like role + the half's name for screen readers.
        let half_label_key = match kind {
            HalfKind::Start => "date-range-edit-start-name",
            HalfKind::End => "date-range-edit-end-name",
        };
        let field_with_a11y = field
            .access_role(Role::DateInput)
            .access_label(resolve_message_widget(half_label_key, &[]));
        let field_id = ctx.add(field_with_a11y);

        // Padding around the field for visual alignment with the
        // separator icon and trigger button.
        let padded_field_id = ctx.add(
            Padding::new(
                field_dims::TEXT_FIELD_PADDING_VERTICAL,
                4.0,
                field_dims::TEXT_FIELD_PADDING_VERTICAL,
                4.0,
            )
            .child_id(field_id),
        );
        // Width policy: start half is always at its natural mask
        // width (so the start date doesn't reflow when only the end
        // changes); end half follows `end_width_policy`. `Default`
        // matches the start (fixed). `Fill` wraps in an
        // `Expand::horizontal()` (zero-basis flex=1) so the end
        // half absorbs the unified frame's leftover width.
        let sized_field_id = match (kind, self.end_width_policy) {
            (HalfKind::End, crate::date_edit::WidthPolicy::Fill) => {
                ctx.add(crate::primitives::Expand::horizontal().child_id(padded_field_id))
            }
            _ => padded_field_id,
        };

        // ── Segment-stepping (Up/Down on focused segment) ──────
        let segment_step: Rc<dyn Fn(i32, &mut EventContext)> = {
            let pattern = pattern_rc.clone();
            let date_signal = date_signal.clone();
            let text_signal = text_signal.clone();
            let other_date = other_date.clone();
            let merge = merge_into_outer.clone();
            Rc::new(move |delta: i32, ctx_evt: &mut EventContext| {
                let pos = caret.get();
                let Some((_, _, kind_seg)) = segment_at_position(&pattern, pos) else {
                    return;
                };
                let current = date_signal.get().unwrap_or_else(today_local);
                let stepped = step_date_field(current, kind_seg, delta);
                let clamped = clamp_date(stepped, min, max);
                date_signal.set(Some(clamped));
                text_signal.set(format_value(&pattern, Some(clamped), None));
                caret_setter(pos);
                merge(Some(clamped), other_date.get(), ctx_evt);
                ctx_evt.request_frame();
            })
        };

        // Attach key preview on a strict ancestor of the field — same
        // pattern DateEdit uses for its ±segment stepping.
        let enabled = self.initial_enabled;
        let read_only = self.read_only;
        let step_for_key = segment_step.clone();

        ctx.add(
            ZStack::new()
                .add_child(sized_field_id)
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
                }),
        )
    }
}

/// Severity rank for `ValidationFeedback`. Higher = more severe.
fn rank(fb: &ValidationFeedback) -> u8 {
    match fb {
        ValidationFeedback::Invalid { .. } => 3,
        ValidationFeedback::Corrected { .. } => 2,
        ValidationFeedback::Valid => 1,
        ValidationFeedback::Pristine => 0,
    }
}

/// Painted right-arrow chevron used as the visual separator
/// between the start and end halves. Same stroke convention as
/// the calendar header chevrons.
fn arrow_right_icon(size: f32) -> IconWidget {
    let mut path = Path::new();
    let s = size;
    // Single chevron pointing right: `>`
    path.move_to(Point::new(s * 0.35, s * 0.20));
    path.line_to(Point::new(s * 0.70, s * 0.50));
    path.line_to(Point::new(s * 0.35, s * 0.80));
    IconWidget::from_path(path, size)
}
