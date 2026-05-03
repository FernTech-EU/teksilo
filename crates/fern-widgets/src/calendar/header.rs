//! `CalendarHeader` — month navigation strip: prev / "Month Year" / next.

use fern_canvas::{Path, Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::accesskit::{Action, Role};
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_i18n::resolve_message_widget;
use fern_tokens::{BorderRole, CornerRadius, SurfaceRole, TextRole, TextStyleRole};

use crate::common::datetime::month_long_key;
use crate::common::datetime::types::YearMonth;
use crate::common::datetime::Date;
use crate::primitives::{Center, Expand, FixedSize, HStack, IconWidget, RectWidget, TextWidget, ZStack};

use super::OnMonthChanged;

pub(crate) struct CalendarHeader {
    visible_month: Signal<YearMonth>,
    focused_date: Signal<Date>,
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
        on_month_changed: Option<OnMonthChanged>,
    ) -> Self {
        Self {
            visible_month,
            focused_date,
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

        // ── Previous year ─────────────────────────────────────
        let visible_for_prev_year = self.visible_month.clone();
        let focused_for_prev_year = self.focused_date.clone();
        let cb_for_prev_year = self.on_month_changed.clone();
        let prev_year_id = ctx.add(NavArrow::new(
            ArrowKind::LeftDouble,
            prev_year_label,
            move |ctx_evt| {
                let new_ym = visible_for_prev_year.get().offset_months(-12);
                visible_for_prev_year.set(new_ym);
                clamp_focus_into_month(&focused_for_prev_year, new_ym);
                if let Some(cb) = cb_for_prev_year.as_ref() {
                    cb(new_ym, ctx_evt);
                }
                ctx_evt.request_frame();
            },
        ));

        // ── Previous month ────────────────────────────────────
        let visible_for_prev = self.visible_month.clone();
        let focused_for_prev = self.focused_date.clone();
        let cb_for_prev = self.on_month_changed.clone();
        let prev_id = ctx.add(NavArrow::new(
            ArrowKind::Left,
            prev_label,
            move |ctx_evt| {
                let new_ym = visible_for_prev.get().prev_month();
                visible_for_prev.set(new_ym);
                clamp_focus_into_month(&focused_for_prev, new_ym);
                if let Some(cb) = cb_for_prev.as_ref() {
                    cb(new_ym, ctx_evt);
                }
                ctx_evt.request_frame();
            },
        ));

        // ── Next month ────────────────────────────────────────
        let visible_for_next = self.visible_month.clone();
        let focused_for_next = self.focused_date.clone();
        let cb_for_next = self.on_month_changed.clone();
        let next_id = ctx.add(NavArrow::new(
            ArrowKind::Right,
            next_label,
            move |ctx_evt| {
                let new_ym = visible_for_next.get().next_month();
                visible_for_next.set(new_ym);
                clamp_focus_into_month(&focused_for_next, new_ym);
                if let Some(cb) = cb_for_next.as_ref() {
                    cb(new_ym, ctx_evt);
                }
                ctx_evt.request_frame();
            },
        ));

        // ── Next year ─────────────────────────────────────────
        let visible_for_next_year = self.visible_month.clone();
        let focused_for_next_year = self.focused_date.clone();
        let cb_for_next_year = self.on_month_changed.clone();
        let next_year_id = ctx.add(NavArrow::new(
            ArrowKind::RightDouble,
            next_year_label,
            move |ctx_evt| {
                let new_ym = visible_for_next_year.get().offset_months(12);
                visible_for_next_year.set(new_ym);
                clamp_focus_into_month(&focused_for_next_year, new_ym);
                if let Some(cb) = cb_for_next_year.as_ref() {
                    cb(new_ym, ctx_evt);
                }
                ctx_evt.request_frame();
            },
        ));

        // Center label "Month Year". Reactively bound so it updates
        // when the visible_month changes.
        let month_signal = self.visible_month.clone();
        let label_signal = month_signal.map(|ym| {
            let month_name = resolve_message_widget(month_long_key(ym.month()), &[]);
            format!("{} {}", month_name, ym.year())
        });
        let month_label = TextWidget::new_literal("")
            .style(TextStyleRole::BodyBold)
            .color(TextRole::Primary)
            .bind_text(label_signal)
            .single_line()
            .a11y_hidden();
        let label_id = ctx.add(month_label);
        let label_centered = ctx.add(Expand::horizontal().child(Center::new().child_id(label_id)));

        let row = HStack::new()
            .spacing(4.0)
            .add_child(prev_year_id)
            .add_child(prev_id)
            .add_child(label_centered)
            .add_child(next_id)
            .add_child(next_year_id);
        let row_id = ctx.add(row);
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
            .corner_radius(CornerRadius::uniform(4.0));
        let bg_id = ctx.add(bg);
        let z = ctx.add(ZStack::new().add_child(bg_id).add_child(centered));
        let sized = ctx.add(FixedSize::new().bind_width(24.0).bind_height(24.0).child_id(z));

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
                if let WidgetEvent::KeyDown { key, .. } = event {
                    if matches!(key, Key::Enter | Key::Space) {
                        key_action(ctx_evt);
                        return EventResponse::Handled;
                    }
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
                .unwrap_or_else(|| Size::new(24.0, 24.0)),
            None => Size::new(24.0, 24.0),
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
