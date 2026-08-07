// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `DateTimeEdit` — single unified control for picking a `DateTime`.
//!
//! Visually one widget: a single bordered frame containing a date
//! `TextInputField` half, a small painted separator, a time
//! `TextInputField` half, and a trailing built-in calendar button that
//! opens a `Calendar` popover anchored below the wrapper. Backed by
//! `Signal<Option<DateTime>>`.
//!
//! ```text
//! ┌──────────────────────────────────────┐
//! │ 05/02/2026   ·   14:35   │ 📅       │
//! └──────────────────────────────────────┘
//! ```
//!
//! # Why one frame?
//!
//! Two adjacent `DateEdit` + `TimeEdit` (one frame each) visually read
//! as two separate fields that happen to be next to each other. A single
//! frame says "this is one moment in time" — same affordance the user
//! is used to from booking sites, calendar apps, and form builders.
//!
//! # Behaviour
//!
//! - **Two text halves** — date pattern on the left (locale-derived
//!   strftime subset), time pattern on the right (24h or 12h, with or
//!   without seconds). Each half carries its own input mask, validator,
//!   and segment-stepping (Up/Down on the focused segment).
//! - **Painted separator** — a thin middle-dot glyph (`·`), no text.
//!   Visual only; AT users see the wrapper's `Role::DateTimeInput`. The
//!   separator can be replaced with a custom string via
//!   `separator` (rendered as styled secondary text).
//! - **One trailing calendar button** — Int UI `IconButton::embedded()` with the
//!   calendar glyph. Opens a single popover hosting `Calendar::single`
//!   bound to the date half. Picking a cell commits the date and closes
//!   the popover; the time half retains whatever the user typed.
//! - **One frame** — focus-aware border (`BorderRole::Focused` while
//!   any half holds focus, otherwise `Default`), validation-aware
//!   border (`Error` for `Invalid`, `Focused` for `Corrected`).
//! - **One validation strip** below the frame — composed feedback from
//!   both halves (worse of the two wins).
//!
//! # Accessibility
//!
//! - Container — `Role::DateTimeInput` with `set_value` formatted as
//!   `YYYY-MM-DDTHH:MM:SS` (ISO 8601 datetime).
//! - Each `TextInputField` keeps its own `Role::TextInput` AT node;
//!   the wrapper's `Role::DateTimeInput` provides the datetime semantics.
//!
//! ```ignore
//! // Requires ctx.signal() — shown as ignore per convention.
//! use teksilo_widgets::date_time_edit::{DateTimeEdit, SecondsMode};
//!
//! let datetime = ctx.signal(None);
//! let _w = DateTimeEdit::new(datetime.clone())
//!     .seconds(SecondsMode::Hidden)
//!     .on_value_changed(|dt, _ctx| println!("{dt:?}"));
//! ```

#[cfg(test)]
mod tests;

use std::rc::Rc;
use teksilo_i18n::lit;
use teksilo_i18n::localized;

use jiff::civil::Weekday;
use teksilo_canvas::{Path, Point, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accesskit::{Action, Role};
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::overlay::{
    DismissBehavior, OverlayDismissCallback, OverlayLayer, OverlayPlacement, OverlayRequest,
};
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::widget::{EventContext, LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::{HandlerSet, WidgetBuilder};
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::resolve_message_widget;
use teksilo_tokens::{BorderRole, CornerRadius, SurfaceRole};

use crate::calendar::Calendar;
use crate::common::datetime::pattern::{
    ParseTarget, ParsedPattern, ParsedValue, format_value, mask_for_pattern, parse_value,
    segment_at_position, step_date_field, step_time_field,
};
use crate::common::datetime::types::today_local;
use crate::common::datetime::{Date, DateTime, Time};
use crate::date_edit::{ValidationBehavior, build_date_validator, calendar_glyph_icon, clamp_date};
use crate::icon_button::{IconButton, IconButtonSize};
use crate::primitives::text_input_field::{TextInputField, ValidationFeedback};
use crate::primitives::{
    Center, FixedSize, HStack, IconWidget, MinSize, Padding, RectWidget, TextWidget, VStack, ZStack,
};
use crate::time_edit::{
    SecondsMode, TimeFormat, build_time_validator, clamp_time, time_pattern_for,
};
use teksilo_i18n::LocalizedString;

type OnValueChanged = Rc<dyn Fn(Option<DateTime>, &mut EventContext)>;

/// Single unified datetime picker over `Signal<Option<DateTime>>`. See
/// the [module docs](self) for the visual layout and behaviour.
pub struct DateTimeEdit {
    value: Signal<Option<DateTime>>,
    /// Internal date half — drives the date `TextInputField` text
    /// signal and is kept in sync with `value` via `ctx.effect`.
    pub(crate) date_part: Signal<Option<Date>>,
    pub(crate) time_part: Signal<Option<Time>>,
    date_text: Signal<String>,
    time_text: Signal<String>,
    /// Set by `::required(Signal<DateTime>)`; wired into `ctx.effect`
    /// in `build()` so observer handles outlive construction.
    required_source: Option<Signal<DateTime>>,
    date_format_pattern: Option<String>,
    /// Explicit 12h/24h override for the time half. `None` (default)
    /// derives from the current locale via `prefers_12_hour_clock`.
    time_format: Option<TimeFormat>,
    seconds: SecondsMode,
    min: Option<DateTime>,
    max: Option<DateTime>,
    step_minutes: u32,
    first_day_of_week: Option<Weekday>,
    show_calendar_button: bool,
    /// Optional separator string between the two halves. When `None`
    /// (default), a thin painted middle-dot glyph is used. When set,
    /// the string is rendered as styled secondary text.
    separator: Option<String>,
    placeholder: LocalizedString,
    /// Enabled state, static or reactive; forwarded to the arena at
    /// build time.
    enabled: Prop<bool>,
    read_only: bool,
    label: Option<LocalizedString>,
    validation_behavior: ValidationBehavior,
    /// How the trailing (time) half claims horizontal space. The
    /// leading (date) half always sizes to its mask-derived natural
    /// width — the date stays put while the time half either matches
    /// that natural width (`WidthPolicy::Default`) or absorbs
    /// extra space (`WidthPolicy::Fill`).
    time_width_policy: crate::date_edit::WidthPolicy,
    /// Composed validation feedback (severity-merged from both halves).
    feedback: Signal<ValidationFeedback>,
    /// `true` while either half holds keyboard focus — drives the
    /// unified frame border.
    focused: Signal<bool>,
    /// `true` while the calendar popover is open — drives the
    /// trigger's AT `set_expanded` and the open/close toggle.
    calendar_popover_open: Signal<bool>,
    on_value_changed: Option<OnValueChanged>,
    style_override: Option<teksilo_core::styles::SharedDateEditStyle>,
    root_child_id: Option<WidgetId>,
    calendar_id: Option<WidgetId>,
    /// Optional plain tooltip text shown after a hover delay. Mutually exclusive
    /// with the rich / composite slots — every setter clears the other two so
    /// the last call wins.
    tooltip_text: Option<LocalizedString>,
    /// Optional rich tooltip source (registry key or inline content).
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Optional composite tooltip body (arbitrary widget tree).
    composite_tooltip_content: Option<Box<dyn Widget>>,
}

impl std::fmt::Debug for DateTimeEdit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DateTimeEdit").finish_non_exhaustive()
    }
}

impl DateTimeEdit {
    /// Create a datetime picker backed by the optional `value` signal.
    pub fn new(value: Signal<Option<DateTime>>) -> Self {
        let initial = value.get();
        let date_part = Signal::new(initial.map(|dt| dt.date()));
        let time_part = Signal::new(initial.map(|dt| dt.time()));
        Self {
            value,
            date_part,
            time_part,
            date_text: Signal::new(String::new()),
            time_text: Signal::new(String::new()),
            required_source: None,
            date_format_pattern: None,
            time_format: None,
            seconds: SecondsMode::Hidden,
            min: None,
            max: None,
            step_minutes: 1,
            first_day_of_week: None,
            show_calendar_button: true,
            separator: None,
            placeholder: LocalizedString::literal(String::new()),
            enabled: Prop::Static(true),
            read_only: false,
            label: None,
            validation_behavior: ValidationBehavior::AutoCorrect,
            time_width_policy: crate::date_edit::WidthPolicy::Default,
            feedback: Signal::new(ValidationFeedback::Pristine),
            focused: Signal::new(false),
            calendar_popover_open: Signal::new(false),
            on_value_changed: None,
            style_override: None,
            root_child_id: None,
            calendar_id: None,
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

    /// Create a datetime picker backed by a *required* (non-optional) signal.
    /// The widget wraps it in an `Option` proxy internally and keeps the two
    /// in sync via `ctx.effect` — the outer signal is never set to `None`.
    pub fn required(value: Signal<DateTime>) -> Self {
        let proxy: Signal<Option<DateTime>> = Signal::new(Some(value.get()));
        let mut s = Self::new(proxy);
        s.required_source = Some(value);
        s
    }

    /// Override the strftime-subset format pattern for the date half
    /// (e.g. `"%d/%m/%Y"`). Defaults to the locale-derived pattern.
    pub fn date_format_pattern(mut self, p: impl Into<String>) -> Self {
        self.date_format_pattern = Some(p.into());
        self
    }

    /// Lock the time half to a specific clock (12h or 24h). When this
    /// builder is *not* called, the time half defaults to the user's
    /// current locale via `prefers_12_hour_clock` — same rule as
    /// standalone `TimeEdit`.
    pub fn time_format(mut self, f: TimeFormat) -> Self {
        self.time_format = Some(f);
        self
    }

    /// Whether the time half includes a seconds field. Defaults to `SecondsMode::Hidden`.
    pub fn seconds(mut self, mode: SecondsMode) -> Self {
        self.seconds = mode;
        self
    }

    /// Earliest selectable datetime (inclusive). Both the calendar cell and the
    /// text validator enforce this floor.
    pub fn min(mut self, dt: DateTime) -> Self {
        self.min = Some(dt);
        self
    }

    /// Latest selectable datetime (inclusive). Both the calendar cell and the
    /// text validator enforce this ceiling.
    pub fn max(mut self, dt: DateTime) -> Self {
        self.max = Some(dt);
        self
    }

    /// Minute increment for Up/Down segment stepping on the minute field.
    /// Defaults to `1`; values below `1` are clamped to `1`.
    pub fn step_minutes(mut self, n: u32) -> Self {
        self.step_minutes = n.max(1);
        self
    }

    /// Override which weekday appears in the first column of the calendar popup.
    pub fn first_day_of_week(mut self, w: Weekday) -> Self {
        self.first_day_of_week = Some(w);
        self
    }

    /// Show or hide the trailing calendar button. Default `true`.
    pub fn show_calendar_button(mut self, show: bool) -> Self {
        self.show_calendar_button = show;
        self
    }

    /// Override the painted middle-dot separator with a custom string
    /// (rendered as styled secondary text between the two halves).
    /// Pass an empty string to suppress the separator entirely.
    pub fn separator(mut self, s: impl Into<String>) -> Self {
        self.separator = Some(s.into());
        self
    }

    /// Placeholder shown when the datetime is `None`.
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

    /// Make both halves read-only; the calendar button is also disabled.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Accessible label for the wrapper `Role::DateTimeInput` node. When not
    /// set, falls back to the localized `date-time-edit-name` message.
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        self.label = Some(ls);
        self
    }

    /// How parse failures are surfaced. Forwarded to both halves —
    /// each half uses the same behaviour.
    pub fn validation_behavior(mut self, behavior: ValidationBehavior) -> Self {
        self.validation_behavior = behavior;
        self
    }

    /// How the trailing (time) half claims horizontal space. The
    /// leading (date) half always sizes to its natural mask width;
    /// the time half follows this policy. Default
    /// `WidthPolicy::Default` (natural width); pass
    /// `WidthPolicy::Fill` to make the time half absorb extra
    /// space the parent offers.
    pub fn time_width_policy(mut self, policy: crate::date_edit::WidthPolicy) -> Self {
        self.time_width_policy = policy;
        self
    }

    /// Reactive handle on the composed validation feedback. Reflects
    /// whichever half is more severe (`Invalid > Corrected > Valid >
    /// Pristine`).
    pub fn validation_feedback_signal(&self) -> Signal<ValidationFeedback> {
        self.feedback.clone()
    }

    /// Callback invoked whenever the datetime changes. Receives the new
    /// `Option<DateTime>` and an `EventContext` for dispatching intents.
    pub fn on_value_changed(
        mut self,
        f: impl Fn(Option<DateTime>, &mut EventContext) + 'static,
    ) -> Self {
        self.on_value_changed = Some(Rc::new(f));
        self
    }

    /// Show a plain single-line tooltip after a hover delay. Mutually
    /// exclusive with `rich_tooltip` / `rich_tooltip_content` /
    /// `composite_tooltip` — each setter clears the other three so the
    /// last call wins.
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Show a rich tooltip identified by a registry key. Mutually
    /// exclusive with `tooltip` / `rich_tooltip_content` /
    /// `composite_tooltip` — each setter clears the other three so the
    /// last call wins.
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Show a rich tooltip with inline content. Mutually exclusive with
    /// `tooltip` / `rich_tooltip` / `composite_tooltip` — each setter
    /// clears the other three so the last call wins.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Show a composite tooltip whose body is an arbitrary widget tree.
    /// Mutually exclusive with `tooltip` / `rich_tooltip` /
    /// `rich_tooltip_content` — each setter clears the other three so
    /// the last call wins.
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }

    /// Clone the underlying `Signal<Option<DateTime>>` for external binding.
    pub fn value(&self) -> Signal<Option<DateTime>> {
        self.value.clone()
    }
}

impl Widget for DateTimeEdit {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme_signal().get();
        use crate::styles::recipe_date_edit_style as de;
        use crate::styles::recipe_text_input_style as field_dims;
        let focus_ring_width = theme.shape.focus_ring_width;
        let self_id = ctx.self_id();
        // Forward the enabled state into the arena; see IconButton.
        ctx.enabled_when(self_id, self.enabled.clone());
        let read_only = self.read_only;

        // ── required-source mirror via ctx.effect ─────────────
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
                    if let Some(dt) = v
                        && src_clone.get() != *dt
                    {
                        src_clone.set(*dt);
                    }
                });
            }
        }

        // ── Resolve patterns ───────────────────────────────────
        let date_pattern_string = self.date_format_pattern.clone().unwrap_or_else(|| {
            let tag = ctx.locale_signal().get().unwrap_or_default();
            crate::common::datetime::format_pattern_for_locale(&tag).to_string()
        });
        let date_pattern = ParsedPattern::parse(&date_pattern_string)
            .unwrap_or_else(|_| ParsedPattern::parse("%Y-%m-%d").unwrap());
        let date_pattern_rc = Rc::new(date_pattern);
        let date_mask = mask_for_pattern(&date_pattern_rc);

        let time_format = self.time_format.unwrap_or_else(|| {
            let tag = ctx.locale_signal().get().unwrap_or_default();
            if crate::common::datetime::prefers_12_hour_clock(&tag) {
                TimeFormat::Hour12
            } else {
                TimeFormat::Hour24
            }
        });
        let time_pattern_string = time_pattern_for(time_format, self.seconds);
        let time_pattern = ParsedPattern::parse(&time_pattern_string)
            .unwrap_or_else(|_| ParsedPattern::parse("%H:%M").unwrap());
        let time_pattern_rc = Rc::new(time_pattern);
        let time_mask = mask_for_pattern(&time_pattern_rc);

        let date_min = self.min.map(|dt| dt.date());
        let date_max = self.max.map(|dt| dt.date());
        let time_min = self.min.map(|dt| dt.time());
        let time_max = self.max.map(|dt| dt.time());

        // ── outer → halves mirror ─────────────────────────────
        // External writes split into halves AND reformat their text so
        // the visible field reflects the new value (programmatic
        // `value.set(...)` should update both the internal date/time
        // signals AND the field's text).
        {
            let date_part = self.date_part.clone();
            let time_part = self.time_part.clone();
            let date_text = self.date_text.clone();
            let time_text = self.time_text.clone();
            let date_pattern = date_pattern_rc.clone();
            let time_pattern = time_pattern_rc.clone();
            ctx.effect(&self.value, move |new_dt| {
                let new_d = new_dt.map(|dt| dt.date());
                let new_t = new_dt.map(|dt| dt.time());
                if date_part.get() != new_d {
                    date_part.set(new_d);
                }
                if time_part.get() != new_t {
                    time_part.set(new_t);
                }
                let d_text = new_d
                    .map(|d| format_value(&date_pattern, Some(d), None))
                    .unwrap_or_default();
                let t_text = new_t
                    .map(|t| format_value(&time_pattern, None, Some(t)))
                    .unwrap_or_default();
                if date_text.get() != d_text {
                    date_text.set(d_text);
                }
                if time_text.get() != t_text {
                    time_text.set(t_text);
                }
            });
        }
        // Seed text once at build time so the initial value is visible
        // without waiting for the first effect tick.
        {
            self.date_text.set(
                self.date_part
                    .get()
                    .map(|d| format_value(&date_pattern_rc, Some(d), None))
                    .unwrap_or_default(),
            );
            self.time_text.set(
                self.time_part
                    .get()
                    .map(|t| format_value(&time_pattern_rc, None, Some(t)))
                    .unwrap_or_default(),
            );
        }

        // ── Build each half as a bare TextInputField ───────────
        // Each half returns (layout wrapper, inner editable field id).
        let (date_field_id, date_inner_id) =
            self.build_date_half(ctx, date_pattern_rc.clone(), &date_mask, date_min, date_max);
        let (time_field_id, time_inner_id) =
            self.build_time_half(ctx, time_pattern_rc.clone(), &time_mask, time_min, time_max);

        // ── Painted (or text) separator ────────────────────────
        // Default: thin painted middle-dot glyph. Apps that want a
        // different shape can pass `.separator("…")` to render that
        // string as styled text instead.
        let separator_id = match self.separator.as_deref() {
            None => {
                let dot = middle_dot_icon(field_dims::TEXT_FIELD_HEIGHT * 0.4)
                    .color(teksilo_tokens::TextRole::Secondary);
                ctx.add(
                    FixedSize::new()
                        .width(field_dims::TEXT_FIELD_HEIGHT * 0.55)
                        .height(field_dims::TEXT_FIELD_HEIGHT)
                        .child(Center::new().child(dot)),
                )
            }
            Some(s) if s.is_empty() => ctx.add(
                FixedSize::new()
                    .width(0.0_f32)
                    .height(field_dims::TEXT_FIELD_HEIGHT),
            ),
            Some(s) => {
                let text = TextWidget::new(lit!(s))
                    .style(teksilo_tokens::TextStyleRole::Body)
                    .color(teksilo_tokens::TextRole::Secondary)
                    .single_line()
                    .a11y_hidden();
                ctx.add(Padding::new(0.0, 6.0, 0.0, 6.0).child(Center::new().child(text)))
            }
        };

        // ── Trailing calendar trigger (date-only) ──────────────
        let trigger_id_opt = if self.show_calendar_button {
            // Bridge signal: the calendar binds to a parallel
            // `Signal<Option<Date>>` so its internal cell-render +
            // arrow-key state can mutate freely; the popover commit
            // path writes the final selection through us.
            let calendar_temp: Signal<Option<Date>> = Signal::new(self.date_part.get());
            {
                let temp = calendar_temp.clone();
                ctx.effect(&self.date_part, move |new_d| {
                    if temp.get() != *new_d {
                        temp.set(*new_d);
                    }
                });
            }
            let popover_open = self.calendar_popover_open.clone();
            let date_part = self.date_part.clone();
            let date_text = self.date_text.clone();
            let date_pattern = date_pattern_rc.clone();
            let value_outer = self.value.clone();
            let time_part = self.time_part.clone();
            let on_changed = self.on_value_changed.clone();
            let return_focus_to = ctx.self_id();
            let mut calendar =
                Calendar::single(calendar_temp.clone()).on_activate(move |d, ctx_evt| {
                    let clamped = clamp_date(d, date_min, date_max);
                    date_part.set(Some(clamped));
                    date_text.set(format_value(&date_pattern, Some(clamped), None));
                    let combined = match (Some(clamped), time_part.get()) {
                        (Some(d), Some(t)) => Some(d.to_datetime(t)),
                        _ => None,
                    };
                    if value_outer.get() != combined {
                        value_outer.set(combined);
                        if let Some(cb) = on_changed.as_ref() {
                            cb(combined, ctx_evt);
                        }
                    }
                    popover_open.set(false);
                    ctx_evt.dismiss_self_overlay_chain();
                    ctx_evt.request_focus(return_focus_to);
                    ctx_evt.request_frame();
                });
            if let Some(min) = date_min {
                calendar = calendar.min_date(min);
            }
            if let Some(max) = date_max {
                calendar = calendar.max_date(max);
            }
            if let Some(fdow) = self.first_day_of_week {
                calendar = calendar.first_day_of_week(fdow);
            }
            let cal_id = ctx.add(calendar);
            ctx.set_dormant(cal_id);
            self.calendar_id = Some(cal_id);

            let popover_open = self.calendar_popover_open.clone();
            let self_ref = ctx.self_id();
            let dismiss_cb: OverlayDismissCallback = {
                let popover_open = popover_open.clone();
                Rc::new(move || {
                    popover_open.set(false);
                })
            };
            let trigger_enabled = self.enabled.as_signal().map(move |on| *on && !read_only);
            let trigger_btn = IconButton::new(calendar_glyph_icon(de::CALENDAR_ICON_SIZE))
                .embedded()
                .size(IconButtonSize::Default)
                .enabled(trigger_enabled)
                .tooltip(localized(move || {
                    resolve_message_widget("date-time-edit-trigger-tooltip", &[])
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
            Some(ctx.add(trigger_btn))
        } else {
            None
        };

        // ── Row layout ─────────────────────────────────────────
        // Each half is wrapped in `Shrinkable` so the row can compress them
        // when the unified frame is narrower than the combined natural mask
        // width — the `TextInputField` inside then scrolls its text instead of
        // overflowing the layout. `Shrinkable` preserves each half's natural
        // width when there's room, so the wide-case layout (date at natural
        // width, time fixed/Fill) is unchanged.
        let date_shrinkable = ctx.add(crate::primitives::Shrinkable::new().child_id(date_field_id));
        let time_shrinkable = ctx.add(crate::primitives::Shrinkable::new().child_id(time_field_id));
        let mut row = HStack::new()
            .spacing(0.0)
            .add_child(date_shrinkable)
            .add_child(separator_id)
            .add_child(time_shrinkable);
        if let Some(trigger_id) = trigger_id_opt {
            row = row.add_child(trigger_id);
        }
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
        // WCAG 3.3.1 / 3.3.3: both editable halves are described by the shared
        // validation message, announced on either when it gains focus.
        ctx.access_described_by(date_inner_id, strip_id);
        ctx.access_described_by(time_inner_id, strip_id);
        // Wrap the frame in `Expand::horizontal().respect_intrinsic()` so it
        // claims the VStack's full width (a VStack lays a child out at its own
        // measured width, not stretched). `respect_intrinsic` keeps the frame's
        // natural width as the basis when unconstrained, so the widget reports
        // its natural mask width rather than collapsing; a bounded proposal
        // narrows it and the `Shrinkable` halves compress to fit.
        let framed_in_vstack = ctx.add(
            crate::primitives::Expand::horizontal()
                .respect_intrinsic()
                .child_id(sized_id),
        );
        let root_with_strip = ctx.add(
            VStack::new()
                .spacing(field_dims::TEXT_FIELD_VALIDATION_STRIP_GAP)
                .add_child(framed_in_vstack)
                .add_child(strip_id),
        );
        let style = crate::styles::recipe_date_edit_style::resolve_date_edit_style(
            &self.style_override,
            ctx,
        );
        let cfg = teksilo_core::styles::DateEditStyleConfig {
            body: root_with_strip,
        };
        let root_id = style.make_body(&cfg, ctx);
        self.root_child_id = Some(root_id);

        // ── Tooltip attachment ─────────────────────────────────
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

        // ── Self handlers: focus_within drives the frame border ─
        let handlers = HandlerSet::new().focus_within(self.focused.clone());
        ctx.apply_self_handlers(handlers);

        // Bind reactive sources at AccessibilityOnly so the wrapper's
        // AT node refreshes set_value / Invalid / set_expanded when
        // the underlying signals change.
        let self_id = ctx.self_id();
        self.value.bind_to(
            self_id,
            ctx.binding_registry(),
            teksilo_core::binding::BindingLevel::AccessibilityOnly,
        );
        self.feedback.bind_to(
            self_id,
            ctx.binding_registry(),
            teksilo_core::binding::BindingLevel::AccessibilityOnly,
        );
        self.calendar_popover_open.bind_to(
            self_id,
            ctx.binding_registry(),
            teksilo_core::binding::BindingLevel::AccessibilityOnly,
        );

        // Return BOTH the visible root AND the dormant calendar
        // popover content as children so the framework links
        // `calendar_id` under this widget in the arena instead of
        // leaving it an orphan root. See popover_widget.rs for the
        // same pattern.
        let mut out = vec![root_with_strip];
        if let Some(cal_id) = self.calendar_id {
            out.push(cal_id);
        }
        out
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // Forward the inner LayoutResponse, then overlay flex=1 when
        // the time half is Fill — the inner HStack consumes the
        // Expand's flex and reports flex=0 to its parent, so the
        // outer wrapper has to advertise flex explicitly.
        let response = match self.root_child_id {
            Some(id) => ctx
                .child_layout_response(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into()),
            None => proposal.resolve(0.0, 0.0).into(),
        };
        if self.time_width_policy == crate::date_edit::WidthPolicy::Fill {
            teksilo_core::widget::LayoutResponse::flexible(response.size, 1.0)
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
        // The visible root fills our bounds; the calendar popover's
        // bounds are owned by the overlay manager when shown
        // (`position_overlays`), so we zero-size it here.
        for child in children.iter_mut() {
            if Some(child.id) == self.calendar_id {
                child.size = teksilo_canvas::Size::ZERO;
                continue;
            }
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        let mut out = Vec::new();
        if let Some(id) = self.root_child_id {
            out.push(id);
        }
        if let Some(id) = self.calendar_id {
            out.push(id);
        }
        out
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::DateTimeInput);
        if let Some(ref label) = self.label {
            builder.set_name(label.clone());
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
                if !self.placeholder.resolve_now().is_empty() {
                    builder.set_placeholder(self.placeholder.resolve_now());
                } else {
                    builder
                        .set_placeholder(resolve_message_widget("date-time-edit-placeholder", &[]));
                }
            }
        }
        // Framework a11y walker sets `set_disabled` from arena state.
        if self.read_only {
            builder.set_read_only();
        }
        if matches!(self.feedback.get(), ValidationFeedback::Invalid { .. }) {
            builder
                .inner_mut()
                .set_invalid(teksilo_core::accesskit::Invalid::True);
        }
        if self.show_calendar_button {
            builder
                .inner_mut()
                .set_has_popup(teksilo_core::accesskit::HasPopup::Grid);
            builder.set_expanded(self.calendar_popover_open.get());
        }
        builder.add_action(Action::Focus);
    }
}

impl DateTimeEdit {
    /// Build the date half as a bare `TextInputField` with mask +
    /// validator + segment-stepping. Returns the WidgetId wrapped in a
    /// fixed-width container so the half visually aligns inside the
    /// unified frame.
    fn build_date_half(
        &self,
        ctx: &mut BuildContext,
        pattern_rc: Rc<ParsedPattern>,
        mask_string: &str,
        min: Option<Date>,
        max: Option<Date>,
    ) -> (WidgetId, WidgetId) {
        let validator =
            build_date_validator(pattern_rc.clone(), min, max, self.validation_behavior);

        let outer_value = self.value.clone();
        let on_changed = self.on_value_changed.clone();
        let time_part = self.time_part.clone();
        let merge_into_outer = move |new_d: Option<Date>, ctx_evt: &mut EventContext| {
            let combined = match (new_d, time_part.get()) {
                (Some(d), Some(t)) => Some(d.to_datetime(t)),
                _ => None,
            };
            if outer_value.get() != combined {
                outer_value.set(combined);
                if let Some(cb) = on_changed.as_ref() {
                    cb(combined, ctx_evt);
                }
            }
        };

        let date_signal = self.date_part.clone();
        let text_signal = self.date_text.clone();

        let commit: Rc<dyn Fn(&mut EventContext)> = {
            let text_signal = text_signal.clone();
            let date_signal = date_signal.clone();
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
                merge(parsed, ctx_evt);
            })
        };

        self.build_field(
            ctx,
            text_signal.clone(),
            mask_string,
            validator,
            self.placeholder.clone(),
            commit,
            "date-time-edit-date-name",
            Role::DateInput,
            DateTimeHalfKind::Date {
                pattern: pattern_rc,
                date_signal,
                text_signal,
                min,
                max,
                merge: Rc::new(merge_into_outer),
            },
        )
    }

    fn build_time_half(
        &self,
        ctx: &mut BuildContext,
        pattern_rc: Rc<ParsedPattern>,
        mask_string: &str,
        min: Option<Time>,
        max: Option<Time>,
    ) -> (WidgetId, WidgetId) {
        let validator =
            build_time_validator(pattern_rc.clone(), min, max, self.validation_behavior);

        let outer_value = self.value.clone();
        let on_changed = self.on_value_changed.clone();
        let date_part = self.date_part.clone();
        let merge_into_outer = move |new_t: Option<Time>, ctx_evt: &mut EventContext| {
            let combined = match (date_part.get(), new_t) {
                (Some(d), Some(t)) => Some(d.to_datetime(t)),
                _ => None,
            };
            if outer_value.get() != combined {
                outer_value.set(combined);
                if let Some(cb) = on_changed.as_ref() {
                    cb(combined, ctx_evt);
                }
            }
        };

        let time_signal = self.time_part.clone();
        let text_signal = self.time_text.clone();

        let commit: Rc<dyn Fn(&mut EventContext)> = {
            let text_signal = text_signal.clone();
            let time_signal = time_signal.clone();
            let pattern = pattern_rc.clone();
            let merge = merge_into_outer.clone();
            Rc::new(move |ctx_evt: &mut EventContext| {
                let raw = text_signal.get();
                let trimmed = raw.trim();
                let parsed: Option<Time> = if trimmed.is_empty() {
                    None
                } else {
                    match parse_value(&pattern, trimmed, ParseTarget::TimeOnly) {
                        Some(ParsedValue::Time(t)) => Some(clamp_time(t, min, max)),
                        _ => time_signal.get(),
                    }
                };
                if time_signal.get() != parsed {
                    time_signal.set(parsed);
                }
                merge(parsed, ctx_evt);
            })
        };

        self.build_field(
            ctx,
            text_signal.clone(),
            mask_string,
            validator,
            LocalizedString::literal(String::new()),
            commit,
            "date-time-edit-time-name",
            Role::TimeInput,
            DateTimeHalfKind::Time {
                pattern: pattern_rc,
                time_signal,
                text_signal,
                min,
                max,
                merge: Rc::new(merge_into_outer),
            },
        )
    }

    /// Shared frame around one half: configures the `TextInputField`
    /// (mask, validator, char filter, commit handlers, a11y), captures
    /// caret accessors for segment-stepping, and wraps in a fixed-width
    /// stepping ancestor that intercepts arrow / page keys.
    #[allow(clippy::too_many_arguments)]
    fn build_field(
        &self,
        ctx: &mut BuildContext,
        text_signal: Signal<String>,
        mask_string: &str,
        validator: crate::primitives::text_input_field::ValidatorFn,
        placeholder: LocalizedString,
        commit: Rc<dyn Fn(&mut EventContext)>,
        a11y_label_key: &str,
        a11y_role: Role,
        kind: DateTimeHalfKind,
    ) -> (WidgetId, WidgetId) {
        use crate::styles::recipe_text_input_style as field_dims;
        let inner_height =
            (field_dims::TEXT_FIELD_HEIGHT - 2.0 * field_dims::TEXT_FIELD_BORDER_WIDTH).max(0.0);
        let text_area_height =
            (inner_height - 2.0 * field_dims::TEXT_FIELD_PADDING_VERTICAL).max(0.0);

        let pattern_for_filter = match &kind {
            DateTimeHalfKind::Date { pattern, .. } => pattern.clone(),
            DateTimeHalfKind::Time { pattern, .. } => pattern.clone(),
        };
        let is_time_half = matches!(kind, DateTimeHalfKind::Time { .. });
        let mut field = TextInputField::new(text_signal.clone())
            .enabled(self.enabled.clone())
            .read_only(self.read_only)
            .placeholder(placeholder)
            .text_height(text_area_height)
            .input_mask(mask_string)
            .validator({
                let v = validator.clone();
                move |s| (v)(s)
            })
            .char_filter(move |c: char| {
                if c.is_ascii_digit() || c == ' ' || c == ':' || c == '-' {
                    return true;
                }
                if is_time_half && matches!(c, 'a' | 'A' | 'p' | 'P' | 'm' | 'M') {
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
        // Mirror this half's feedback into the composed feedback signal
        // (worse-of-two — both halves install this and the worse always
        // wins because each effect computes max(self, current composed)).
        {
            let inner_feedback = field.validation_feedback_signal();
            let composed = self.feedback.clone();
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

        let caret = field.caret_position();
        let caret_setter = field.caret_setter();

        let field_with_a11y = field
            .access_role(a11y_role)
            .access_label(resolve_message_widget(a11y_label_key, &[]));
        let field_id = ctx.add(field_with_a11y);

        let padded_field_id = ctx.add(
            Padding::new(
                field_dims::TEXT_FIELD_PADDING_VERTICAL,
                4.0,
                field_dims::TEXT_FIELD_PADDING_VERTICAL,
                4.0,
            )
            .child_id(field_id),
        );
        // Width policy: date (leading) half is always at its natural
        // mask width; time (trailing) half follows `time_width_policy`.
        // `Default` matches the date — the time stays fixed. `Fill`
        // wraps in `Expand::horizontal()` (zero-basis flex=1) so the
        // time half absorbs the unified frame's leftover width.
        let is_time = matches!(kind, DateTimeHalfKind::Time { .. });
        let sized_field_id =
            if is_time && self.time_width_policy == crate::date_edit::WidthPolicy::Fill {
                ctx.add(crate::primitives::Expand::horizontal().child_id(padded_field_id))
            } else {
                padded_field_id
            };

        // ── Segment-stepping (Up/Down/PageUp/PageDown on focused
        //    segment) ─────────────────────────────────────────
        let segment_step: Rc<dyn Fn(i32, &mut EventContext)> = match kind {
            DateTimeHalfKind::Date {
                pattern,
                date_signal,
                text_signal,
                min,
                max,
                merge,
            } => {
                let caret = caret.clone();
                let caret_setter = caret_setter.clone();
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
                    merge(Some(clamped), ctx_evt);
                    ctx_evt.request_frame();
                })
            }
            DateTimeHalfKind::Time {
                pattern,
                time_signal,
                text_signal,
                min,
                max,
                merge,
            } => {
                let caret = caret.clone();
                let caret_setter = caret_setter.clone();
                Rc::new(move |delta: i32, ctx_evt: &mut EventContext| {
                    let pos = caret.get();
                    let Some((_, _, kind_seg)) = segment_at_position(&pattern, pos) else {
                        return;
                    };
                    let current = time_signal.get().unwrap_or_else(Time::midnight);
                    let stepped = step_time_field(current, kind_seg, delta);
                    let clamped = clamp_time(stepped, min, max);
                    time_signal.set(Some(clamped));
                    text_signal.set(format_value(&pattern, None, Some(clamped)));
                    caret_setter(pos);
                    merge(Some(clamped), ctx_evt);
                    ctx_evt.request_frame();
                })
            }
        };

        // No manual `enabled` gate here: dispatch is already centrally
        // gated by `arena.is_enabled()` (walking up from the focused
        // field through this ZStack to the composite root's
        // `enabled_when`) before any handler — including
        // `on_key_preview` — runs.
        let read_only = self.read_only;
        let step_for_key = segment_step.clone();
        let stepping_id = ctx.add(ZStack::new().add_child(sized_field_id).on_key_preview(
            move |event, ctx_evt| {
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
            },
        ));
        // Return both the outer stepping wrapper (used for layout) and the
        // inner editable field id, so the caller can wire `described_by` onto
        // the node that actually carries `Role::{Date,Time}Input`.
        (stepping_id, field_id)
    }
}

/// Per-half data passed into `build_field` so the segment-step closure
/// can be specialised for date vs time without reaching back into
/// `self` through extra clones at every Up/Down keystroke.
enum DateTimeHalfKind {
    Date {
        pattern: Rc<ParsedPattern>,
        date_signal: Signal<Option<Date>>,
        text_signal: Signal<String>,
        min: Option<Date>,
        max: Option<Date>,
        merge: Rc<dyn Fn(Option<Date>, &mut EventContext)>,
    },
    Time {
        pattern: Rc<ParsedPattern>,
        time_signal: Signal<Option<Time>>,
        text_signal: Signal<String>,
        min: Option<Time>,
        max: Option<Time>,
        merge: Rc<dyn Fn(Option<Time>, &mut EventContext)>,
    },
}

/// Severity rank for `ValidationFeedback`. Higher = more severe.
/// Re-exported via `compose_feedback` for the test module.
pub(crate) fn rank(fb: &ValidationFeedback) -> u8 {
    match fb {
        ValidationFeedback::Invalid { .. } => 3,
        ValidationFeedback::Corrected { .. } => 2,
        ValidationFeedback::Valid => 1,
        ValidationFeedback::Pristine => 0,
    }
}

/// Pick the more severe of two halves. `Invalid > Corrected > Valid >
/// Pristine`. Currently only used by the test module — the live
/// composition path inlines the same `max-by-rank` merge inside each
/// half's feedback effect for clarity.
#[cfg(test)]
pub(crate) fn compose_feedback(
    a: &ValidationFeedback,
    b: &ValidationFeedback,
) -> ValidationFeedback {
    if rank(a) >= rank(b) {
        a.clone()
    } else {
        b.clone()
    }
}

/// Painted middle-dot glyph used as the visual separator between the
/// date and time halves. Same stroke convention as `DateRangeEdit`'s
/// arrow chevron — sized off the field height.
fn middle_dot_icon(size: f32) -> IconWidget {
    let mut path = Path::new();
    let s = size;
    let cx = s * 0.5;
    let cy = s * 0.5;
    let r = s * 0.10;
    // Approximate a small filled circle with two cubic-ish curves via
    // four straight-line segments forming a diamond. Tiny enough that
    // the diamond reads as a dot at typical glyph sizes.
    path.move_to(Point::new(cx, cy - r));
    path.line_to(Point::new(cx + r, cy));
    path.line_to(Point::new(cx, cy + r));
    path.line_to(Point::new(cx - r, cy));
    path.close();
    IconWidget::from_path(path, size)
}
