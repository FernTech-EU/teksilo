//! `MonthsGrid` + `YearsGrid` — the coarser-grain bodies the calendar
//! shows when [`CalendarMode`](super::CalendarMode) is `Months` or
//! `Years`. Both are 4×3 grids of clickable cells:
//!
//! - [`MonthsGrid`] → 12 localized month names (Jan..Dec). Clicking a
//!   cell sets the visible_month to that month + zooms back to
//!   [`CalendarMode::Days`].
//! - [`YearsGrid`] → 12 years of the current decade (e.g. 2020..2031
//!   for a calendar showing 2026). Clicking a cell sets the
//!   visible_month's year to that year + zooms back to
//!   [`CalendarMode::Months`].
//!
//! Cells are minimal: hover + pressed + selected backgrounds, no per-
//! cell focus ring (header-zoom is primarily a click-driven shortcut;
//! deep keyboard nav inside zoom modes is deferred).

use std::rc::Rc;

use fern_canvas::{Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::accesskit::{Action, Role};
use fern_core::build_context::BuildContext;
use fern_core::color_prop::ColorProp;
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_i18n::resolve_message_widget;
use fern_tokens::{CornerRadius, SurfaceRole, TextRole, TextStyleRole};

use crate::common::datetime::month_long_key;
use crate::common::datetime::types::YearMonth;
use crate::primitives::{HStack, RectWidget, TextWidget, VStack, ZStack};

use super::CalendarMode;

const COLUMNS: usize = 3;
const ROWS: usize = 4;
const CELL_SPACING: f32 = 4.0;
const CELL_RADIUS: f32 = 6.0;

// ── MonthsGrid ───────────────────────────────────────────────────────

pub(crate) struct MonthsGrid {
    visible_month: Signal<YearMonth>,
    mode: Signal<CalendarMode>,
    enabled: bool,
    cell_height: f32,
    root_id: Option<WidgetId>,
}

impl std::fmt::Debug for MonthsGrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MonthsGrid").finish()
    }
}

impl MonthsGrid {
    pub(crate) fn new(
        visible_month: Signal<YearMonth>,
        mode: Signal<CalendarMode>,
        enabled: bool,
        cell_height: f32,
    ) -> Self {
        Self {
            visible_month,
            mode,
            enabled,
            cell_height,
            root_id: None,
        }
    }
}

impl Widget for MonthsGrid {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let mut rows: Vec<WidgetId> = Vec::with_capacity(ROWS);
        for r in 0..ROWS {
            let mut row = HStack::new().spacing(CELL_SPACING);
            for c in 0..COLUMNS {
                let month = (r * COLUMNS + c + 1) as i8;
                let visible_month_for_cell = self.visible_month.clone();
                let mode_for_cell = self.mode.clone();
                let label = resolve_message_widget(month_long_key(month), &[]);
                let selected_signal = visible_month_for_cell
                    .map(move |ym| ym.month() == month);
                let cell = ZoomCell::new(
                    label,
                    selected_signal,
                    self.enabled,
                    self.cell_height,
                    Rc::new(move |ctx_evt: &mut EventContext| {
                        let cur = visible_month_for_cell.get();
                        visible_month_for_cell.set(YearMonth::new(cur.year(), month));
                        mode_for_cell.set(CalendarMode::Days);
                        ctx_evt.request_frame();
                    }),
                );
                let cell_id = ctx.add(cell);
                // Wrap each cell in Expand so the COLUMNS cells share
                // the row's width equally. Without this the HStack
                // would lay them out at their natural text width and
                // they'd bunch up on the leading edge.
                let expanded_cell_id = ctx.add(
                    crate::primitives::Expand::horizontal().child_id(cell_id),
                );
                row = row.add_child(expanded_cell_id);
            }
            rows.push(ctx.add(row));
        }
        let mut col = VStack::new().spacing(CELL_SPACING);
        for id in rows {
            col = col.add_child(id);
        }
        let root = ctx.add(col);
        self.root_id = Some(root);
        vec![root]
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
        builder.set_role(Role::Grid);
        builder.set_name(resolve_message_widget("calendar-months-grid-label", &[]));
    }
}

// ── YearsGrid ────────────────────────────────────────────────────────

pub(crate) struct YearsGrid {
    visible_month: Signal<YearMonth>,
    mode: Signal<CalendarMode>,
    enabled: bool,
    cell_height: f32,
    root_id: Option<WidgetId>,
}

impl std::fmt::Debug for YearsGrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("YearsGrid").finish()
    }
}

impl YearsGrid {
    pub(crate) fn new(
        visible_month: Signal<YearMonth>,
        mode: Signal<CalendarMode>,
        enabled: bool,
        cell_height: f32,
    ) -> Self {
        Self {
            visible_month,
            mode,
            enabled,
            cell_height,
            root_id: None,
        }
    }

    /// Decade containing `year`. Returns `(decade_start, decade_end)`
    /// where the 12-cell grid covers `decade_start - 1` (faint, last
    /// year of previous decade) through `decade_start + 10`. The
    /// faint-edge cells help orient the user — same convention as
    /// the day grid's leading/trailing out-of-month cells.
    pub(crate) fn decade_of(year: i16) -> i16 {
        // 2026 → 2020; 2020 → 2020; 2030 → 2030.
        (year / 10) * 10
    }
}

impl Widget for YearsGrid {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let visible = self.visible_month.get();
        let decade_start = Self::decade_of(visible.year());
        // 12 cells: decade_start - 1 .. decade_start + 11
        let mut rows: Vec<WidgetId> = Vec::with_capacity(ROWS);
        for r in 0..ROWS {
            let mut row = HStack::new().spacing(CELL_SPACING);
            for c in 0..COLUMNS {
                let cell_year = decade_start - 1 + (r * COLUMNS + c) as i16;
                let visible_for_cell = self.visible_month.clone();
                let mode_for_cell = self.mode.clone();
                let label = format!("{cell_year}");
                let selected_signal = visible_for_cell
                    .map(move |ym| ym.year() == cell_year);
                let cell = ZoomCell::new(
                    label,
                    selected_signal,
                    self.enabled,
                    self.cell_height,
                    Rc::new(move |ctx_evt: &mut EventContext| {
                        let cur = visible_for_cell.get();
                        visible_for_cell.set(YearMonth::new(cell_year, cur.month()));
                        mode_for_cell.set(CalendarMode::Months);
                        ctx_evt.request_frame();
                    }),
                );
                let cell_id = ctx.add(cell);
                // Wrap each cell in Expand so the COLUMNS cells share
                // the row's width equally (same fix as `MonthsGrid`).
                let expanded_cell_id = ctx.add(
                    crate::primitives::Expand::horizontal().child_id(cell_id),
                );
                row = row.add_child(expanded_cell_id);
            }
            rows.push(ctx.add(row));
        }
        let mut col = VStack::new().spacing(CELL_SPACING);
        for id in rows {
            col = col.add_child(id);
        }
        let root = ctx.add(col);
        self.root_id = Some(root);
        vec![root]
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
        builder.set_role(Role::Grid);
        builder.set_name(resolve_message_widget("calendar-years-grid-label", &[]));
    }
}

// ── ZoomCell (shared by both grids) ──────────────────────────────────

struct ZoomCell {
    label: String,
    selected: Signal<bool>,
    enabled: bool,
    height: f32,
    on_pick: Rc<dyn Fn(&mut EventContext)>,
    root_id: Option<WidgetId>,
}

impl std::fmt::Debug for ZoomCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZoomCell").field("label", &self.label).finish()
    }
}

impl ZoomCell {
    fn new(
        label: String,
        selected: Signal<bool>,
        enabled: bool,
        height: f32,
        on_pick: Rc<dyn Fn(&mut EventContext)>,
    ) -> Self {
        Self {
            label,
            selected,
            enabled,
            height,
            on_pick,
            root_id: None,
        }
    }
}

impl Widget for ZoomCell {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let hover = ctx.signal(false);
        let pressed = ctx.signal(false);
        let selected = self.selected.clone();

        // Background role priorities: selected wins (accent fill) →
        // pressed → hover → transparent. Same role precedence the
        // day grid uses.
        let bg_role = selected
            .clone()
            .zip3(&hover, &pressed)
            .map(|(sel, hov, prs)| {
                if *sel {
                    SurfaceRole::Accent
                } else if *prs {
                    SurfaceRole::Pressed
                } else if *hov {
                    SurfaceRole::Hover
                } else {
                    SurfaceRole::Transparent
                }
            });
        let text_role = selected.map(|sel| {
            if *sel {
                TextRole::OnAccent
            } else {
                TextRole::Primary
            }
        });

        let bg = RectWidget::new()
            .background(ColorProp::DynamicSurfaceRole(bg_role))
            .corner_radius(CornerRadius::uniform(CELL_RADIUS));
        let bg_id = ctx.add(bg);

        let text = TextWidget::new_literal(self.label.clone())
            .style(TextStyleRole::Body)
            .color(ColorProp::DynamicTextRole(text_role))
            .single_line();
        let text_id = ctx.add(crate::primitives::Center::new().child(text));

        let z = ZStack::new().add_child(bg_id).add_child(text_id);
        let z_id = ctx.add(z);
        let sized = ctx.add(
            crate::primitives::FixedSize::new()
                .bind_height(self.height)
                .child_id(z_id),
        );

        let label_for_a11y = self.label.clone();
        let on_pick = self.on_pick.clone();
        let on_pick_for_access = on_pick.clone();
        let hover_for_handler = hover.clone();
        let pressed_for_handler = pressed.clone();
        let pressed_for_release = pressed.clone();
        let enabled = self.enabled;
        let handlers = HandlerSet::new()
            .focusable(enabled)
            .cursor(CursorIcon::Pointer)
            .on_hover(move |entered, _| {
                hover_for_handler.set(entered);
                if !entered {
                    pressed_for_release.set(false);
                }
            })
            .on_tap(move |_pos, ctx_evt| {
                if !enabled {
                    return;
                }
                pressed_for_handler.set(false);
                (on_pick)(ctx_evt);
            })
            .on_access_action(move |action, ctx_evt| {
                use fern_core::event::EventResponse;
                if action == Action::Click {
                    (on_pick_for_access)(ctx_evt);
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            });
        ctx.apply_self_handlers(handlers);
        // We attach to self; use the inner `sized` as the visible body.
        self.root_id = Some(sized);
        // Stash the label for accessibility().
        let _ = label_for_a11y;
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
        builder.set_role(Role::GridCell);
        builder.set_name(self.label.clone());
        if self.selected.get() {
            builder.set_selected(true);
        }
    }
}
