// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `MonthsGrid` + `YearsGrid` — the coarser-grain bodies the calendar
//! shows when [`CalendarMode`] is `Months` or
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
//! Visual chrome (selected / pressed / hover background, label
//! colour) is delegated to the active `CalendarStyle::make_zoom_cell`.
//!
//! ## Keyboard and assistive technology
//!
//! The cursor of a zoomed view is the month or year the calendar shows,
//! the cell painted selected. The keys are the calendar's, handled by
//! [`Zoom::handle_key`] while a zoomed view is up, and the cells are no Tab
//! stops: the calendar root keeps focus and names the cell under the cursor
//! as its active descendant, which is how a day of the day grid is heard.
//! Each cell advertises `Action::Click`, so a screen reader can pick it.
//!
//! ## Touch and pen
//!
//! A zoom cell is well past the target floor on both axes at every density, and it
//! picks on the release. Its **pressed** appearance is the framework's press now:
//! the cell used to write `false` into that signal and nothing else, so the state
//! its `CalendarStyle` painted was unreachable for a mouse as well as for a finger.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accesskit::{Action, Role};
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key};
use teksilo_core::signal::Signal;
use teksilo_core::styles::{CalendarZoomCellConfig, SharedCalendarStyle};
use teksilo_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::{LanguageIdentifier, resolve_message_widget};

use crate::common::datetime::Date;
use crate::common::datetime::month_long_key;
use crate::common::datetime::types::YearMonth;
use crate::common::datetime::written::month_and_year;
use crate::primitives::{HStack, VStack};
use crate::styles::recipe_calendar_style::RecipeCalendarStyle;

use super::{CalendarMode, OnMonthChanged, clamp_to_month};
use teksilo_core::styles::density::spacing;
use teksilo_tokens::InputTokens;

const COLUMNS: usize = 3;
const ROWS: usize = 4;
const CELL_SPACING: f32 = 4.0;

/// [`CELL_SPACING`] scaled by the density's `spacing_factor`
/// (1.00 / 1.15 / 1.30).
fn cell_spacing(tokens: &InputTokens) -> f32 {
    spacing(CELL_SPACING, tokens)
}

/// The cells a zoomed view built, each with the month number (months view)
/// or the year (years view) it stands for, rewritten by every build of the
/// view. The calendar root names the one under the cursor from here.
pub(crate) type ZoomCells = Rc<RefCell<Vec<(i16, WidgetId)>>>;

/// The cell `cells` holds for `key`, if the view built one.
pub(crate) fn cell_for(cells: &ZoomCells, key: i16) -> Option<WidgetId> {
    cells
        .borrow()
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, id)| *id)
}

/// The calendar state the zoomed views read and move.
#[derive(Clone)]
pub(crate) struct Zoom {
    pub(crate) visible_month: Signal<YearMonth>,
    pub(crate) focused_date: Signal<Date>,
    pub(crate) mode: Signal<CalendarMode>,
}

impl Zoom {
    /// Zoom in one level from `view` on what the cursor is on: a year opens
    /// on its months, a month on its days. The day cursor goes into the
    /// month opened, on the same day of the month where it has one, so the
    /// day grid has a day to name as the new focus.
    fn zoom_in(&self, view: CalendarMode) {
        match view {
            CalendarMode::Years => self.mode.set(CalendarMode::Months),
            CalendarMode::Months | CalendarMode::Days => self.show_days(),
        }
    }

    /// The day view, on the month shown, the day cursor moved into it.
    fn show_days(&self) {
        let month = self.visible_month.get();
        let cursor = self.focused_date.get();
        if YearMonth::from_date(cursor) != month
            && let Some(day) = clamp_to_month(cursor, month)
        {
            self.focused_date.set(day);
        }
        self.mode.set(CalendarMode::Days);
    }

    /// A key pressed while the months or years view is up.
    ///
    /// The arrows move the cursor (the month or year shown) across a row by
    /// one and between rows by three, the grids being three wide; PageUp and
    /// PageDown by a year in the months view and a decade in the years view.
    /// Enter and Space zoom in on the cursor, and Escape returns to the days.
    /// None of them selects anything.
    pub(crate) fn handle_key(
        &self,
        key: Key,
        view: CalendarMode,
        on_month_changed: Option<&OnMonthChanged>,
        ctx: &mut EventContext,
    ) -> EventResponse {
        let unit = match view {
            CalendarMode::Years => 12,
            CalendarMode::Months | CalendarMode::Days => 1,
        };
        let months = match key {
            Key::ArrowLeft => -unit,
            Key::ArrowRight => unit,
            Key::ArrowUp => -(COLUMNS as i32) * unit,
            Key::ArrowDown => COLUMNS as i32 * unit,
            Key::PageUp => -12 * unit,
            Key::PageDown => 12 * unit,
            Key::Enter | Key::Space => {
                self.zoom_in(view);
                ctx.request_frame();
                return EventResponse::Handled;
            }
            Key::Escape => {
                self.show_days();
                ctx.request_frame();
                return EventResponse::Handled;
            }
            _ => return EventResponse::Ignored,
        };
        let cur = self.visible_month.get();
        let next = cur.offset_months(months);
        if next != cur {
            self.visible_month.set(next);
            if let Some(cb) = on_month_changed {
                cb(next, ctx);
            }
        }
        ctx.request_frame();
        EventResponse::Handled
    }
}

// ── MonthsGrid ───────────────────────────────────────────────────────

pub(crate) struct MonthsGrid {
    zoom: Zoom,
    cells: ZoomCells,
    /// The locale a month's name is written in, with its year.
    lang: LanguageIdentifier,
    enabled: bool,
    cell_width: f32,
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
        zoom: Zoom,
        cells: ZoomCells,
        lang: LanguageIdentifier,
        enabled: bool,
        cell_width: f32,
        cell_height: f32,
    ) -> Self {
        Self {
            zoom,
            cells,
            lang,
            enabled,
            cell_width,
            cell_height,
            root_id: None,
        }
    }
}

impl Widget for MonthsGrid {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let mut rows: Vec<WidgetId> = Vec::with_capacity(ROWS);
        let mut cells = Vec::with_capacity(ROWS * COLUMNS);
        for r in 0..ROWS {
            let mut row = HStack::new().spacing(cell_spacing(&ctx.theme().input));
            for c in 0..COLUMNS {
                let month = (r * COLUMNS + c + 1) as i8;
                let zoom = self.zoom.clone();
                let label = resolve_message_widget(month_long_key(month), &[]);
                let selected_signal = self.zoom.visible_month.map(move |ym| ym.month() == month);
                // Named with its year, as a day is named with its month and
                // year: the cursor crosses into the next year from December,
                // and PageDown changes the year alone, and the name is what
                // says so.
                let lang = self.lang.clone();
                let name = self
                    .zoom
                    .visible_month
                    .map(move |ym| month_and_year(YearMonth::new(ym.year(), month), &lang));
                let cell = ZoomCell::new(
                    label,
                    name,
                    selected_signal,
                    self.enabled,
                    self.cell_width,
                    self.cell_height,
                    Rc::new(move |ctx_evt: &mut EventContext| {
                        let cur = zoom.visible_month.get();
                        zoom.visible_month.set(YearMonth::new(cur.year(), month));
                        zoom.zoom_in(CalendarMode::Months);
                        ctx_evt.request_frame();
                    }),
                );
                let cell_id = ctx.add(cell);
                cells.push((i16::from(month), cell_id));
                row = row.child(cell_id);
            }
            rows.push(ctx.add(row));
        }
        *self.cells.borrow_mut() = cells;
        let mut col = VStack::new().spacing(cell_spacing(&ctx.theme().input));
        for id in rows {
            col = col.child(id);
        }
        let root = ctx.add(col);
        self.root_id = Some(root);
        vec![root]
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
        builder.set_role(Role::Grid);
        builder.set_name(resolve_message_widget("calendar-months-grid-label", &[]));
    }
}

// ── YearsGrid ────────────────────────────────────────────────────────

pub(crate) struct YearsGrid {
    zoom: Zoom,
    cells: ZoomCells,
    /// The first year of the decade the grid was built for, bound at
    /// `Rebuild`: a cursor or a header arrow that leaves the decade lays the
    /// grid out again for the one it entered. The grid used to be laid out
    /// once, and a year past the first decade shown had no cell.
    decade: Signal<i16>,
    enabled: bool,
    cell_width: f32,
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
        zoom: Zoom,
        cells: ZoomCells,
        enabled: bool,
        cell_width: f32,
        cell_height: f32,
    ) -> Self {
        let decade = Signal::new(Self::decade_of(zoom.visible_month.get().year()));
        Self {
            zoom,
            cells,
            decade,
            enabled,
            cell_width,
            cell_height,
            root_id: None,
        }
    }

    /// Decade containing `year`. Returns `decade_start`, where the
    /// 12-cell grid covers `decade_start - 1` (faint, last
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
        let visible = self.zoom.visible_month.get();
        let decade_start = Self::decade_of(visible.year());
        if self.decade.get() != decade_start {
            self.decade.set(decade_start);
        }
        {
            let decade = self.decade.clone();
            ctx.effect(&self.zoom.visible_month, move |ym| {
                let entered = Self::decade_of(ym.year());
                if decade.get() != entered {
                    decade.set(entered);
                }
            });
        }
        self.decade
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);
        // 12 cells: decade_start - 1 .. decade_start + 11
        let mut rows: Vec<WidgetId> = Vec::with_capacity(ROWS);
        let mut cells = Vec::with_capacity(ROWS * COLUMNS);
        for r in 0..ROWS {
            let mut row = HStack::new().spacing(cell_spacing(&ctx.theme().input));
            for c in 0..COLUMNS {
                let cell_year = decade_start - 1 + (r * COLUMNS + c) as i16;
                let zoom = self.zoom.clone();
                let label = format!("{cell_year}");
                let selected_signal = self
                    .zoom
                    .visible_month
                    .map(move |ym| ym.year() == cell_year);
                let cell = ZoomCell::new(
                    label.clone(),
                    Signal::new(label),
                    selected_signal,
                    self.enabled,
                    self.cell_width,
                    self.cell_height,
                    Rc::new(move |ctx_evt: &mut EventContext| {
                        let cur = zoom.visible_month.get();
                        zoom.visible_month
                            .set(YearMonth::new(cell_year, cur.month()));
                        zoom.zoom_in(CalendarMode::Years);
                        ctx_evt.request_frame();
                    }),
                );
                let cell_id = ctx.add(cell);
                cells.push((cell_year, cell_id));
                row = row.child(cell_id);
            }
            rows.push(ctx.add(row));
        }
        *self.cells.borrow_mut() = cells;
        let mut col = VStack::new().spacing(cell_spacing(&ctx.theme().input));
        for id in rows {
            col = col.child(id);
        }
        let root = ctx.add(col);
        self.root_id = Some(root);
        vec![root]
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
        builder.set_role(Role::Grid);
        builder.set_name(resolve_message_widget("calendar-years-grid-label", &[]));
    }
}

// ── ZoomCell (shared by both grids) ──────────────────────────────────

struct ZoomCell {
    label: String,
    /// What the cell is called: a month with its year, or a year.
    name: Signal<String>,
    selected: Signal<bool>,
    enabled: bool,
    cell_width: f32,
    cell_height: f32,
    on_pick: Rc<dyn Fn(&mut EventContext)>,
    root_id: Option<WidgetId>,
}

impl std::fmt::Debug for ZoomCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZoomCell")
            .field("label", &self.label)
            .finish()
    }
}

impl ZoomCell {
    fn new(
        label: String,
        name: Signal<String>,
        selected: Signal<bool>,
        enabled: bool,
        cell_width: f32,
        cell_height: f32,
        on_pick: Rc<dyn Fn(&mut EventContext)>,
    ) -> Self {
        Self {
            label,
            name,
            selected,
            enabled,
            cell_width,
            cell_height,
            on_pick,
            root_id: None,
        }
    }
}

impl Widget for ZoomCell {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let hover = ctx.signal(false);
        let pressed = ctx.signal(false);
        // The pressed chrome the `CalendarStyle` paints, driven by the router's
        // own press record — the cell has no press handler of its own to keep.
        // Before this it only ever wrote `false`: the month and year cells had
        // a pressed appearance no pointer of any kind could reach. The
        // framework press is also the only source that survives a press
        // sliding off the cell and back on, and that is withdrawn without a
        // release when an enclosing scroller claims the pan.
        crate::common::interaction::bind_pressed(ctx, pressed.clone());
        let selected = self.selected.clone();

        // Visual chrome via the active CalendarStyle.
        let style = resolve_calendar_style(ctx);
        let cfg = CalendarZoomCellConfig {
            label: self.label.clone(),
            is_selected: selected,
            is_hovered: hover.clone(),
            is_pressed: pressed.clone(),
            cell_width: self.cell_width,
            cell_height: self.cell_height,
        };
        let chrome_id = style.make_zoom_cell(&cfg, ctx);

        let on_pick = self.on_pick.clone();
        let on_pick_for_access = on_pick.clone();
        let hover_for_handler = hover.clone();
        let enabled = self.enabled;
        // No Tab stop: the calendar root keeps focus and names the cell under
        // its cursor, and twelve stops, one a month from January, were what
        // Tab went through here before.
        let handlers = HandlerSet::new()
            .focusable(false)
            .cursor(CursorIcon::Pointer)
            .on_hover(move |entered, _| {
                hover_for_handler.set(entered);
            })
            .on_tap(move |_pos, ctx_evt| {
                if !enabled {
                    return;
                }
                (on_pick)(ctx_evt);
            })
            .on_access_action(move |action, ctx_evt| {
                if action == Action::Click && enabled {
                    (on_pick_for_access)(ctx_evt);
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            });
        ctx.apply_self_handlers(handlers);
        self.name.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::AccessibilityOnly,
        );
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
        builder.set_name(self.name.get());
        if self.selected.get() {
            builder.set_selected(true);
        }
        // The pick was serviced and never advertised, so no screen reader
        // offered it: AT-SPI lists no action on a node that advertises none.
        // No `Action::Focus`, for the reason a day has none (see `DayCell`).
        builder.add_action(Action::Click);
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
