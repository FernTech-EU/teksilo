// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `DayCell` — single day cell in a calendar's day grid.
//!
//! Owns the date-bound state computation (today / out-of-month /
//! disabled / fill role derived from the calendar's `SelectionBinding`)
//! and the tap pipeline. Visual chrome (background fill, today ring,
//! roving-focus ring, day-number label) is delegated to the active
//! `CalendarStyle::make_day_cell` via `cfg`.

use std::rc::Rc;

use teksilo_canvas::{Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accesskit::{Action, Role};
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::EventResponse;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{CalendarDayConfig, CalendarDayFill, SharedCalendarStyle};
use teksilo_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::LanguageIdentifier;

use crate::common::datetime::Date;
use crate::common::datetime::types::{YearMonth, today_local};
use crate::common::datetime::written::full_date;
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
    /// The locale the cell's name is written in; the calendar's.
    lang: LanguageIdentifier,
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
        lang: LanguageIdentifier,
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
            lang,
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
        // `:focus-visible`: the roving ring shows only during keyboard
        // navigation — gate on the live input-modality signal so clicking a
        // day selects it without painting the roving focus indicator.
        let is_focused_cell = self
            .focused_date
            .zip(&calendar_focused)
            .map(move |(focused_d, has_focus)| *has_focus && *focused_d == date_owned)
            .and(&ctx.focus_visible());

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

        // One selection closure, two entry points: a pointer tap and an
        // AT / automation `Action::Click`. The cell advertises
        // `Action::Click` in `accessibility`, and the dispatcher never
        // synthesizes a tap from it, so the AT path has to run the same
        // pipeline explicitly. `interactable` is checked inside (this
        // cell is not arena-disabled — the guard lives here).
        let select_date: Rc<dyn Fn(&mut EventContext)> = Rc::new(move |ctx_evt| {
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
            ctx_evt.request_frame();
        });

        let handlers = HandlerSet::new()
            .focusable(false) // grid uses roving focus on the parent widget
            .cursor(if interactable {
                CursorIcon::Pointer
            } else {
                CursorIcon::Default
            })
            .on_tap({
                let select_date = select_date.clone();
                move |_pos, ctx_evt: &mut EventContext| select_date(ctx_evt)
            })
            .on_access_action(move |action, ctx_evt: &mut EventContext| {
                if action == Action::Click {
                    select_date(ctx_evt);
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            });
        ctx.apply_self_handlers(handlers);

        self.root_id = Some(chrome_id);
        vec![chrome_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
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

        // The day in full, written by ICU in the locale's order and grammar:
        // "samedi 2 mai 2026", "Saturday, May 2, 2026". Never assembled from
        // translated weekday and month names, which put a French day after
        // its month and leave a Russian month in the nominative.
        builder.set_name(full_date(self.date, &self.lang));

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
        // indicator. The keyboard cursor is not a state of the cell either:
        // the calendar root names the cell under it as its active
        // descendant, which makes that cell the platform's focus (see
        // `Calendar::accessibility`).
        if self.is_today {
            builder.set_aria_current(teksilo_core::accesskit::AriaCurrent::Date);
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
        // No `Action::Focus`: a day is reached through the grid's active
        // descendant, never focused itself, as a grouped `RadioTile` is. The
        // dispatcher services `Focus` by moving keyboard focus onto the node
        // it names, focusable or not (`pointer_router.rs`), so an assistive
        // technology that focused a day (a UIA `SetFocus`, VoiceOver's
        // keyboard focus following its cursor) took focus off the grid. The
        // grid then named no descendant, and every arrow press after it moved
        // the cursor in silence, the platform's focus left on that day.
    }
}

fn resolve_calendar_style(ctx: &BuildContext) -> SharedCalendarStyle {
    ctx.theme_signal()
        .get()
        .style_slots
        .calendar
        .clone()
        .unwrap_or_else(|| {
            Rc::new(RecipeCalendarStyle::for_tokens(&ctx.theme().input)) as SharedCalendarStyle
        })
}
