//! `DayCell` — single day cell in a calendar's day grid.
//!
//! Owns the date-bound state computation (today / out-of-month /
//! disabled / fill role derived from the calendar's `SelectionBinding`)
//! and the tap pipeline. Visual chrome (background fill, today ring,
//! roving-focus ring, day-number label) is delegated to the active
//! `CalendarStyle::make_day_cell` via `cfg`.

use std::rc::Rc;

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::accesskit::{Action, Role};
use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::styles::{CalendarDayConfig, CalendarDayFill, SharedCalendarStyle};
use fern_core::widget::{CursorIcon, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_i18n::resolve_message_widget;

use crate::common::datetime::Date;
use crate::common::datetime::types::{YearMonth, today_local};
use crate::common::datetime::{month_long_key, weekday_long_key};
use crate::styles::recipe_calendar_style::RecipeCalendarStyle;

use super::{
    DisabledDateFilter, OnActivate, OnRangeChanged, OnSelectionChanged, SelectionBinding,
    commit_date, is_date_disabled,
};

#[allow(clippy::too_many_arguments)]
pub(crate) struct DayCell {
    date: Date,
    visible_month: Signal<YearMonth>,
    focused_date: Signal<Date>,
    /// `true` while the parent Calendar holds keyboard focus. Combined
    /// with `focused_date == self.date` to render the roving-focus
    /// ring only when the user is actively keyboard-navigating.
    calendar_focused: Signal<bool>,
    selection: SelectionBinding,
    cell_size: f32,
    min_date: Option<Date>,
    max_date: Option<Date>,
    disabled_filter: Option<DisabledDateFilter>,
    enabled: bool,
    on_selection_changed: Option<OnSelectionChanged>,
    on_range_changed: Option<OnRangeChanged>,
    on_activate: Option<OnActivate>,
    range_status: Signal<String>,
    root_id: Option<WidgetId>,
    is_today: bool,
}

impl std::fmt::Debug for DayCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DayCell").field("date", &self.date).finish()
    }
}

impl DayCell {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        date: Date,
        visible_month: Signal<YearMonth>,
        focused_date: Signal<Date>,
        calendar_focused: Signal<bool>,
        selection: SelectionBinding,
        cell_size: f32,
        min_date: Option<Date>,
        max_date: Option<Date>,
        disabled_filter: Option<DisabledDateFilter>,
        enabled: bool,
        on_selection_changed: Option<OnSelectionChanged>,
        on_range_changed: Option<OnRangeChanged>,
        on_activate: Option<OnActivate>,
        range_status: Signal<String>,
    ) -> Self {
        let today = today_local();
        Self {
            date,
            visible_month,
            focused_date,
            calendar_focused,
            selection,
            cell_size,
            min_date,
            max_date,
            disabled_filter,
            enabled,
            on_selection_changed,
            on_range_changed,
            on_activate,
            range_status,
            root_id: None,
            is_today: date == today,
        }
    }
}

impl Widget for DayCell {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let date = self.date;
        let in_visible_month = YearMonth::from_date(date) == self.visible_month.get();

        let disabled_static = is_date_disabled(
            date,
            self.min_date,
            self.max_date,
            self.disabled_filter.as_ref(),
        );
        let interactable = self.enabled && !disabled_static;

        // ── Selection-derived fill (reactive) ──────────────────
        // Drives the recipe's background colour without rebuilding
        // the cell on selection changes.
        let fill: Signal<CalendarDayFill> = match &self.selection {
            SelectionBinding::Single(sig) => {
                let date_owned = date;
                sig.map(move |sel| match sel {
                    Some(d) if *d == date_owned => CalendarDayFill::Selected,
                    _ => CalendarDayFill::None,
                })
            }
            SelectionBinding::Range { value, anchor } => {
                let date_owned = date;
                let v = value.clone();
                let a = anchor.clone();
                v.zip(&a).map(move |(rng, anc)| {
                    if let Some(start) = anc {
                        // Mid-selection — show anchor as Selected.
                        if date_owned == *start {
                            return CalendarDayFill::Selected;
                        }
                    }
                    if let Some(rng) = rng {
                        if date_owned == rng.start || date_owned == rng.end {
                            CalendarDayFill::Selected
                        } else if rng.contains(date_owned) {
                            CalendarDayFill::InRange
                        } else {
                            CalendarDayFill::None
                        }
                    } else {
                        CalendarDayFill::None
                    }
                })
            }
        };

        // ── Roving focus indicator — only visible while the parent
        // calendar holds keyboard focus AND this cell is the focused
        // date. The recipe binds visibility on the focus-ring node.
        let date_owned = date;
        let calendar_focused = self.calendar_focused.clone();
        let is_focused_cell = self
            .focused_date
            .zip(&calendar_focused)
            .map(move |(focused_d, has_focus)| *has_focus && *focused_d == date_owned);

        // ── Delegate visual chrome to the active CalendarStyle ─
        let style = resolve_calendar_style(ctx);
        let cfg = CalendarDayConfig {
            label: format!("{}", date.day()),
            fill,
            is_today: self.is_today,
            is_out_of_month: !in_visible_month,
            is_disabled: disabled_static,
            is_focused_cell,
            cell_size: self.cell_size,
        };
        let chrome_id = style.make_day_cell(&cfg, ctx);

        // ── Tap handler ────────────────────────────────────────
        let date_owned = date;
        let selection = self.selection.clone();
        let on_sel = self.on_selection_changed.clone();
        let on_range = self.on_range_changed.clone();
        let on_activate = self.on_activate.clone();
        let visible_month = self.visible_month.clone();
        let focused_date = self.focused_date.clone();
        let range_status = self.range_status.clone();

        let handlers = HandlerSet::new()
            .focusable(false) // grid uses roving focus on the parent widget
            .cursor(if interactable {
                CursorIcon::Pointer
            } else {
                CursorIcon::Default
            })
            .on_tap(move |_pos, ctx_evt| {
                if !interactable {
                    return;
                }
                // Move focus to this cell first.
                focused_date.set(date_owned);
                // If clicking outside the visible month, follow.
                let new_ym = YearMonth::from_date(date_owned);
                if visible_month.get() != new_ym {
                    visible_month.set(new_ym);
                }
                commit_date(
                    date_owned,
                    &selection,
                    on_sel.as_ref(),
                    on_range.as_ref(),
                    on_activate.as_ref(),
                    ctx_evt,
                );
                update_range_status(&selection, &range_status);
                ctx_evt.request_frame();
            });
        ctx.apply_self_handlers(handlers);

        self.root_id = Some(chrome_id);
        vec![chrome_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        match self.root_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| Size::new(self.cell_size, self.cell_size)),
            None => Size::new(self.cell_size, self.cell_size),
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
        builder.set_role(Role::GridCell);

        let weekday_label = resolve_message_widget(weekday_long_key(self.date.weekday()), &[]);
        let month_label = resolve_message_widget(month_long_key(self.date.month()), &[]);
        let name = format!(
            "{} {} {}, {}",
            weekday_label,
            month_label,
            self.date.day(),
            self.date.year()
        );
        builder.set_name(name);

        // Selected state.
        let date = self.date;
        let selected = match &self.selection {
            SelectionBinding::Single(sig) => sig.get() == Some(date),
            SelectionBinding::Range { value, .. } => {
                value.get().map(|r| r.contains(date)).unwrap_or(false)
            }
        };
        builder.set_selected(selected);

        // `aria-current="date"` per ARIA spec marks the date that
        // represents *today* in a calendar — not the keyboard-focus
        // indicator. The roving-focus visualization is announced via
        // the parent calendar's Live region (see `set_live(Polite)`
        // on the Calendar root and the focused-cell announcement
        // wired in `Calendar::build`).
        if self.is_today {
            builder.set_aria_current(fern_core::accesskit::AriaCurrent::Date);
        }

        if is_date_disabled(
            self.date,
            self.min_date,
            self.max_date,
            self.disabled_filter.as_ref(),
        ) || !self.enabled
        {
            builder.set_disabled();
        }

        builder.add_action(Action::Click);
        builder.add_action(Action::Focus);
    }
}

fn resolve_calendar_style(ctx: &BuildContext) -> SharedCalendarStyle {
    ctx.theme_signal()
        .get()
        .style_slots
        .calendar
        .clone()
        .unwrap_or_else(|| Rc::new(RecipeCalendarStyle) as SharedCalendarStyle)
}

fn update_range_status(selection: &SelectionBinding, status: &Signal<String>) {
    if let SelectionBinding::Range { value, .. } = selection {
        let s = match value.get() {
            Some(r) => format!(
                "{:04}-{:02}-{:02} – {:04}-{:02}-{:02}",
                r.start.year(),
                r.start.month(),
                r.start.day(),
                r.end.year(),
                r.end.month(),
                r.end.day(),
            ),
            None => String::new(),
        };
        if status.get() != s {
            status.set(s);
        }
    }
}
