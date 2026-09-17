// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `CalendarHeader` — month navigation strip: prev / "Month Year" / next.
//!
//! ## Touch and pen
//!
//! The nav arrows' footprint follows the density ladder, like the day cells beside
//! them — they read the raw constant before, so a Touch build had 24 dp arrows in a
//! header whose recipe had already decided on 44. The "Month Year" title is a
//! `Button` and takes its floor from the button recipe.

use std::rc::Rc;
use teksilo_i18n::lit;

use teksilo_canvas::{Path, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accesskit::{Action, Role};
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::signal::Signal;
use teksilo_core::styles::{CalendarHeaderConfig, CalendarStyle, SharedCalendarStyle};
use teksilo_core::widget::{CursorIcon, LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::resolve_message_widget;
use teksilo_tokens::{BorderRole, CornerRadius, SurfaceRole};

use crate::common::conformance_box::{conformance_box, conformance_box_size};
use crate::common::datetime::Date;
use crate::common::datetime::month_long_key;
use crate::common::datetime::types::YearMonth;
use crate::primitives::{Center, FixedSize, IconWidget, RectWidget, ZStack};
use crate::styles::recipe_calendar_style::{CALENDAR_NAV_ARROW_RADIUS, RecipeCalendarStyle};

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
                move |dir: i32, ctx_evt: &mut teksilo_core::widget::EventContext| {
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
                move |dir: i32, ctx_evt: &mut teksilo_core::widget::EventContext| {
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
        // "2020 — 2029"). Reactive via `Button::label`, so
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
        let title_btn = crate::button::Button::new(lit!(""))
            .label(label_signal)
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
    ) -> teksilo_core::widget::LayoutResponse {
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
    on_activate: std::rc::Rc<dyn Fn(&mut teksilo_core::widget::EventContext)>,
    root_id: Option<WidgetId>,
    /// The node extent `build` resolved, in logical pixels — the conformance
    /// box's when one was built, the painted chrome's when the box was the
    /// identity (the same number `conformance_box` hands back either way).
    ///
    /// [`Widget::hit_outset`] is handed a pointer kind and the token ladder and
    /// nothing else — no theme, no text scale — so the size it has to make up
    /// the shortfall from is parked here when it is decided. A build-time read
    /// cannot go stale: a density change marks the tree at
    /// `BindingLevel::Rebuild`, and so does a text-scale change, which is the
    /// same reason `HeaderCell::zone_floor` is a `Cell`.
    extent: std::cell::Cell<f32>,
}

impl std::fmt::Debug for NavArrow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NavArrow").finish()
    }
}

impl NavArrow {
    fn new<F>(kind: ArrowKind, label: String, f: F) -> Self
    where
        F: Fn(&mut teksilo_core::widget::EventContext) + 'static,
    {
        Self {
            kind,
            label,
            on_activate: std::rc::Rc::new(f),
            root_id: None,
            extent: std::cell::Cell::new(0.0),
        }
    }
}

impl Widget for NavArrow {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Grow the nav arrows with the global text scale (the enclosing Calendar
        // rebuilds on a scale change, so a build-time read is sufficient).
        let scale = ctx.text_scale();
        let icon = match self.kind {
            ArrowKind::Left => chevron_left_icon(12.0 * scale),
            ArrowKind::Right => IconWidget::chevron_right(12.0 * scale),
            ArrowKind::LeftDouble => double_chevron_left_icon(12.0 * scale),
            ArrowKind::RightDouble => double_chevron_right_icon(12.0 * scale),
        };
        let icon_id = ctx.add(icon);
        let centered = ctx.add(Center::new().child(icon_id));

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
            .corner_radius(CornerRadius::uniform(CALENDAR_NAV_ARROW_RADIUS * scale));
        let bg_id = ctx.add(bg);
        let z = ctx.add(ZStack::new().child(bg_id).child(centered));
        // The arrow's painted footprint comes from the active `CalendarStyle`,
        // which is what lets a preset set it — macOS's `NSDatePicker` stepper is
        // 20 dp, and the arrow used to render the shipped 24 whatever the
        // theme had decided. The default accessor is still the density ladder
        // (24 dp at Compact — the identity — 32 at Comfortable, 44 at Touch),
        // and the global text scale multiplies whichever number comes back.
        let painted = nav_arrow_extent(ctx.theme()) * scale;
        // …and the *node* is the conformance box over that chrome (see
        // `common::conformance_box` for the bargain). The reason it is a box
        // here rather than only a `hit_outset` is that an outset cannot escape
        // its parent: the first and last arrows sit flush against the header
        // row's own edge, so their outer sliver has nowhere to grow into and
        // they stayed non-conformant with the outset alone. Under Int UI —
        // whose arrow is the density's target at every rung — the box is the
        // identity and the helper adds no nodes at all.
        let chrome = ctx.add(FixedSize::new().width(painted).height(painted).child(z));
        let (sized, box_size) = conformance_box(ctx, chrome, Size::new(painted, painted));
        self.extent.set(box_size.width);

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
    ) -> teksilo_core::widget::LayoutResponse {
        // The *box*, not the painted chrome — see `build`. Only a fallback:
        // the mounted arrow answers from its child. Shares the helper's
        // arithmetic so the built extent and the fallback cannot disagree.
        let side = nav_arrow_extent(ctx.theme);
        let fallback = conformance_box_size(
            Size::new(side, side),
            ctx.theme.input.min_target_conformance,
        );
        match self.root_id {
            Some(id) => ctx.child_size(id, proposal).unwrap_or(fallback),
            None => fallback,
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

    /// Top the arrow's box up to the density's target size for a coarse
    /// pointer, between the pointer and the arena.
    ///
    /// The **conformance** floor is the box's job — `build` hands the chrome
    /// to `common::conformance_box`, which grows the node to the floor and
    /// centres the preset's chrome inside it — because an outset cannot
    /// escape its parent and the outermost arrows sit flush against the header
    /// row's edge. What is left for an outset is the part above that floor,
    /// for a finger or a pen, where growing the box would move the header's
    /// layout for a mouse user too.
    ///
    /// So this is the coarse-pointer top-up and nothing else, and
    /// [`target_outset`](crate::button::target_outset) — which is zero for a
    /// precise pointer — says exactly that. Conformance is the box's job, not
    /// this one's: the box is what `a_calendars_nav_arrows_clear_the_
    /// conformance_floor_at_every_density` holds in the macOS preset's tests,
    /// and splitting the guarantee across two mechanisms would leave neither
    /// owning it.
    fn hit_outset(
        &self,
        kind: teksilo_tokens::PointerKind,
        tokens: &teksilo_tokens::InputTokens,
    ) -> teksilo_canvas::EdgeInsets {
        let extent = self.extent.get();
        crate::button::target_outset(Size::new(extent, extent), kind, tokens)
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

/// The nav arrow's footprint under `theme`: the active [`CalendarStyle`]'s
/// `nav_arrow_size`, or the shipped recipe's when no slot is installed.
///
/// Takes a whole `&Theme` rather than `&InputTokens` because the answer is the
/// *style's*, not the density's alone — and because `layout_response` reaches
/// the theme through a `LayoutContext`, where no `BuildContext` exists. Both
/// call sites go through here so the built footprint and the measured one
/// cannot disagree.
fn nav_arrow_extent(theme: &teksilo_core::styles::Theme) -> f32 {
    match &theme.style_slots.calendar {
        Some(style) => style.nav_arrow_size(&theme.input),
        None => RecipeCalendarStyle::for_tokens(&theme.input).nav_arrow_size(&theme.input),
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
