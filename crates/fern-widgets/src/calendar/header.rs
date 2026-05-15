//! `CalendarHeader` — month navigation strip: prev / "Month Year" / next.

use std::rc::Rc;

use fern_canvas::{Path, Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::accesskit::{Action, Role};
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::styles::{CalendarHeaderConfig, SharedCalendarStyle};
use fern_core::widget::{CursorIcon, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_i18n::resolve_message_widget;
use fern_tokens::{BorderRole, CornerRadius, SurfaceRole};

use crate::common::datetime::Date;
use crate::common::datetime::month_long_key;
use crate::common::datetime::types::YearMonth;
use crate::primitives::{Center, FixedSize, IconWidget, RectWidget, ZStack};
use crate::styles::recipe_calendar_style::{
    CALENDAR_NAV_ARROW_RADIUS, CALENDAR_NAV_ARROW_SIZE, RecipeCalendarStyle,
};

use super::{CalendarMode, OnMonthChanged};

pub(crate) struct CalendarHeader {
    visible_month: Signal<YearMonth>,
    focused_date: Signal<Date>,
    /// Body mode driving (a) what unit the chevrons step and
    /// (b) the title-button's demote target.
    mode: Signal<CalendarMode>,
    on_month_changed: Option<OnMonthChanged>,
    root_id: Option<WidgetId>,
}

impl std::fmt::Debug for CalendarHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CalendarHeader").finish()
    }
}

impl CalendarHeader {
    pub(crate) fn new(
        visible_month: Signal<YearMonth>,
        focused_date: Signal<Date>,
        mode: Signal<CalendarMode>,
        on_month_changed: Option<OnMonthChanged>,
    ) -> Self {
        Self {
            visible_month,
            focused_date,
            mode,
            on_month_changed,
            root_id: None,
        }
    }
}

impl Widget for CalendarHeader {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let prev_year_label = resolve_message_widget("calendar-button-previous-year", &[]);
        let next_year_label = resolve_message_widget("calendar-button-next-year", &[]);
        let prev_label = resolve_message_widget("calendar-button-previous-month", &[]);
        let next_label = resolve_message_widget("calendar-button-next-month", &[]);

        // Mode-aware step helpers. Single-chevron steps the visible
        // body's natural unit (1 month in Days, 1 year in Months,
        // 10 years in Years). Double-chevron steps a coarser unit
        // (1 year, 10 years, 100 years respectively).
        let step_single = {
            let visible = self.visible_month.clone();
            let focused = self.focused_date.clone();
            let mode = self.mode.clone();
            let cb = self.on_month_changed.clone();
            std::rc::Rc::new(
                move |dir: i32, ctx_evt: &mut fern_core::widget::EventContext| {
                    let cur = visible.get();
                    let new_ym = match mode.get() {
                        CalendarMode::Days => {
                            if dir > 0 {
                                cur.next_month()
                            } else {
                                cur.prev_month()
                            }
                        }
                        CalendarMode::Months => cur.offset_months(dir * 12),
                        CalendarMode::Years => cur.offset_months(dir * 120),
                    };
                    visible.set(new_ym);
                    clamp_focus_into_month(&focused, new_ym);
                    if let Some(cb) = cb.as_ref() {
                        cb(new_ym, ctx_evt);
                    }
                    ctx_evt.request_frame();
                },
            )
        };
        let step_double = {
            let visible = self.visible_month.clone();
            let focused = self.focused_date.clone();
            let mode = self.mode.clone();
            let cb = self.on_month_changed.clone();
            std::rc::Rc::new(
                move |dir: i32, ctx_evt: &mut fern_core::widget::EventContext| {
                    let cur = visible.get();
                    let new_ym = match mode.get() {
                        CalendarMode::Days => cur.offset_months(dir * 12),
                        CalendarMode::Months => cur.offset_months(dir * 120),
                        // Years: a "double" step = +/- 100 years, but
                        // YearMonth saturates so this is a soft jump.
                        CalendarMode::Years => cur.offset_months(dir * 1200),
                    };
                    visible.set(new_ym);
                    clamp_focus_into_month(&focused, new_ym);
                    if let Some(cb) = cb.as_ref() {
                        cb(new_ym, ctx_evt);
                    }
                    ctx_evt.request_frame();
                },
            )
        };

        // ── Previous double (year / decade / century) ─────────
        let step_dbl_prev = step_double.clone();
        let prev_year_id = ctx.add(NavArrow::new(
            ArrowKind::LeftDouble,
            prev_year_label,
            move |ctx_evt| step_dbl_prev(-1, ctx_evt),
        ));

        // ── Previous single (month / year / decade) ───────────
        let step_sgl_prev = step_single.clone();
        let prev_id = ctx.add(NavArrow::new(ArrowKind::Left, prev_label, move |ctx_evt| {
            step_sgl_prev(-1, ctx_evt)
        }));

        // ── Next single (month / year / decade) ───────────────
        let step_sgl_next = step_single.clone();
        let next_id = ctx.add(NavArrow::new(
            ArrowKind::Right,
            next_label,
            move |ctx_evt| step_sgl_next(1, ctx_evt),
        ));

        // ── Next double (year / decade / century) ─────────────
        let step_dbl_next = step_double.clone();
        let next_year_id = ctx.add(NavArrow::new(
            ArrowKind::RightDouble,
            next_year_label,
            move |ctx_evt| step_dbl_next(1, ctx_evt),
        ));

        // Center label — a Flat Button bound to a derived label
        // signal (mode + visible_month → "May 2026" / "2026" /
        // "2020 — 2029"). Reactive via `Button::bind_label`, so
        // the calendar doesn't have to rebuild on mode flips.
        let label_signal = self.visible_month.zip(&self.mode).map(|(ym, m)| match m {
            CalendarMode::Days => {
                let month_name = resolve_message_widget(month_long_key(ym.month()), &[]);
                format!("{} {}", month_name, ym.year())
            }
            CalendarMode::Months => format!("{}", ym.year()),
            CalendarMode::Years => {
                let start = (ym.year() / 10) * 10;
                format!("{} — {}", start, start + 9)
            }
        });
        let mode_for_action = self.mode.clone();
        let title_btn = crate::button::Button::new_literal("")
            .bind_label(label_signal)
            .variant(crate::button::ButtonVariant::Ghost)
            .on_activate_fn(move |ctx_evt| {
                let cur = mode_for_action.get();
                let next = cur.demote();
                if next != cur {
                    mode_for_action.set(next);
                    ctx_evt.request_frame();
                }
            });
        let title_id = ctx.add(title_btn);

        // Delegate row layout to the active CalendarStyle.
        let style = resolve_calendar_style(ctx);
        let cfg = CalendarHeaderConfig {
            prev_double: Some(prev_year_id),
            prev: Some(prev_id),
            title: title_id,
            next: Some(next_id),
            next_double: Some(next_year_id),
        };
        let row_id = style.make_header(&cfg, ctx);
        self.root_id = Some(row_id);
        vec![row_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        match self.root_id {
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
        self.root_id.into_iter().collect()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::Group);
        builder.set_hidden();
    }
}

// ── Single icon-only navigation arrow (prev/next) ────────────────────

#[derive(Clone, Copy)]
enum ArrowKind {
    Left,
    Right,
    /// Double-left chevron (« style) — prev year.
    LeftDouble,
    /// Double-right chevron (» style) — next year.
    RightDouble,
}

struct NavArrow {
    kind: ArrowKind,
    label: String,
    on_activate: std::rc::Rc<dyn Fn(&mut fern_core::widget::EventContext)>,
    root_id: Option<WidgetId>,
}

impl std::fmt::Debug for NavArrow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NavArrow").finish()
    }
}

impl NavArrow {
    fn new<F>(kind: ArrowKind, label: String, f: F) -> Self
    where
        F: Fn(&mut fern_core::widget::EventContext) + 'static,
    {
        Self {
            kind,
            label,
            on_activate: std::rc::Rc::new(f),
            root_id: None,
        }
    }
}

impl Widget for NavArrow {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let icon = match self.kind {
            ArrowKind::Left => chevron_left_icon(12.0),
            ArrowKind::Right => IconWidget::chevron_right(12.0),
            ArrowKind::LeftDouble => double_chevron_left_icon(12.0),
            ArrowKind::RightDouble => double_chevron_right_icon(12.0),
        };
        let icon_id = ctx.add(icon);
        let centered = ctx.add(Center::new().child_id(icon_id));

        // Focus state drives the Int UI accent border on focus. No
        // hover/pressed roles needed for these tiny chrome buttons —
        // the accent border is the keyboard-affordance signal.
        let focused = ctx.signal(false);
        let focus_ring_width = ctx.theme_signal().get().shape.focus_ring_width;
        let border_role = focused.map(|f| {
            if *f {
                BorderRole::Focused
            } else {
                BorderRole::Transparent
            }
        });
        let border_width = focused.map(move |f| if *f { focus_ring_width } else { 0.0 });

        let bg = RectWidget::new()
            .background(SurfaceRole::Transparent)
            .border_color(border_role)
            .border_width(border_width)
            .corner_radius(CornerRadius::uniform(CALENDAR_NAV_ARROW_RADIUS));
        let bg_id = ctx.add(bg);
        let z = ctx.add(ZStack::new().add_child(bg_id).add_child(centered));
        let sized = ctx.add(
            FixedSize::new()
                .bind_width(CALENDAR_NAV_ARROW_SIZE)
                .bind_height(CALENDAR_NAV_ARROW_SIZE)
                .child_id(z),
        );

        // Activation needs to fire from pointer click (`on_tap`),
        // keyboard Enter / Space when the button is focused
        // (`on_key`), and AT-invoked Action::Click (`on_access_action`).
        // All three route through the same closure to keep behavior
        // consistent across input modalities.
        let tap_action = self.on_activate.clone();
        let key_action = self.on_activate.clone();
        let access_action = self.on_activate.clone();
        let focused_for_handler = focused.clone();
        let handlers = HandlerSet::new()
            .focusable(true)
            .on_focus(move |has_focus, _ctx| {
                focused_for_handler.set(has_focus);
            })
            .cursor(CursorIcon::Pointer)
            .on_tap(move |_pos, ctx_evt| tap_action(ctx_evt))
            .on_key(move |event, ctx_evt| {
                if let WidgetEvent::KeyDown { key, .. } = event
                    && matches!(key, Key::Enter | Key::Space)
                {
                    key_action(ctx_evt);
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            })
            .on_access_action(move |action, ctx_evt| {
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
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        match self.root_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| Size::new(CALENDAR_NAV_ARROW_SIZE, CALENDAR_NAV_ARROW_SIZE)),
            None => Size::new(CALENDAR_NAV_ARROW_SIZE, CALENDAR_NAV_ARROW_SIZE),
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
        builder.set_name(&self.label);
        builder.add_action(Action::Click);
        builder.add_action(Action::Focus);
    }
}

/// Move the focused-date signal into `new_ym` if it isn't there
/// already, clamping the day to the new month's last valid day. Used
/// by all four nav buttons so changing month/year keeps the roving
/// focus on a real day in the visible page.
fn clamp_focus_into_month(focused: &Signal<Date>, new_ym: YearMonth) {
    let cur = focused.get();
    if YearMonth::from_date(cur) == new_ym {
        return;
    }
    let day = cur.day().min(new_ym.last_day().day());
    if let Ok(d) = Date::new(new_ym.year(), new_ym.month(), day) {
        focused.set(d);
    }
}

fn chevron_left_icon(size: f32) -> IconWidget {
    let mut path = Path::new();
    let s = size;
    path.move_to(Point::new(s * 0.65, s * 0.25));
    path.line_to(Point::new(s * 0.35, s * 0.5));
    path.line_to(Point::new(s * 0.65, s * 0.75));
    IconWidget::from_path(path, size)
}

/// Two left chevrons side by side: « — used for "previous year".
fn double_chevron_left_icon(size: f32) -> IconWidget {
    let mut path = Path::new();
    let s = size;
    // Outer (left) chevron tip at x=0.20, joint at x=0.50
    path.move_to(Point::new(s * 0.50, s * 0.25));
    path.line_to(Point::new(s * 0.20, s * 0.50));
    path.line_to(Point::new(s * 0.50, s * 0.75));
    // Inner (right) chevron tip at x=0.50, joint at x=0.80
    path.move_to(Point::new(s * 0.80, s * 0.25));
    path.line_to(Point::new(s * 0.50, s * 0.50));
    path.line_to(Point::new(s * 0.80, s * 0.75));
    IconWidget::from_path(path, size)
}

/// Two right chevrons side by side: » — used for "next year".
fn double_chevron_right_icon(size: f32) -> IconWidget {
    let mut path = Path::new();
    let s = size;
    // Inner (left) chevron tip at x=0.50, joint at x=0.20
    path.move_to(Point::new(s * 0.20, s * 0.25));
    path.line_to(Point::new(s * 0.50, s * 0.50));
    path.line_to(Point::new(s * 0.20, s * 0.75));
    // Outer (right) chevron tip at x=0.80, joint at x=0.50
    path.move_to(Point::new(s * 0.50, s * 0.25));
    path.line_to(Point::new(s * 0.80, s * 0.50));
    path.line_to(Point::new(s * 0.50, s * 0.75));
    IconWidget::from_path(path, size)
}

fn resolve_calendar_style(ctx: &BuildContext) -> SharedCalendarStyle {
    ctx.theme_signal()
        .get()
        .style_slots
        .calendar
        .clone()
        .unwrap_or_else(|| Rc::new(RecipeCalendarStyle) as SharedCalendarStyle)
}
