// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `Calendar`. See `docs/styling-system.md`.
//!
//! Multi-method trait because `Calendar` paints three distinct cell
//! shapes, each with its own state semantics:
//!
//! * **Day cell** (`make_day_cell`) — the 6×7 grid of day numbers.
//!   Static state (today / out-of-month / disabled) is computed at
//!   build time; the selection-derived fill role and the roving-focus
//!   ring are reactive signals so navigation doesn't rebuild 42 cells.
//! * **Zoom cell** (`make_zoom_cell`) — the 4×3 month / year picker
//!   cells. Hover and pressed are reactive (cells handle their own
//!   pointer state); `selected` is reactive against the visible month.
//! * **Header** (`make_header`) — the prev-double / prev / title /
//!   next / next-double row. The four arrow buttons and the title
//!   button are pre-built (the widget computes the mode-aware step
//!   callbacks); the style only lays them out.
//!
//! Outer chrome (the calendar's background frame + padding) is a thin
//! popover-style surface and stays widget-owned via raw shape tokens —
//! not an additional style method.

use std::rc::Rc;

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

/// Selection-derived fill state for a day cell. The reactive signal in
/// `CalendarDayConfig` recomputes on every selection change without
/// re-`build()`ing the cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CalendarDayFill {
    /// No selection touches this cell.
    #[default]
    None,
    /// This cell is the single-selection target *or* a range endpoint.
    Selected,
    /// This cell falls inside a multi-day range, not on an endpoint.
    InRange,
}

/// Per-day cell input to `CalendarStyle::make_day_cell`.
///
/// The static fields (`is_today`, `is_out_of_month`, `is_disabled`)
/// don't change within a build session — the calendar binds
/// `visible_month` at `Rebuild` level, so cells are regenerated when
/// the visible month moves. Everything reactive (selection fill, the
/// roving-focus ring) flows through `Signal`s so per-click navigation
/// doesn't tear down the 42-cell subtree.
///
/// `label` is passed as a plain string rather than a pre-built widget
/// id so the recipe owns the label's text-colour binding — the label
/// colour depends on the reactive fill state (Selected → `OnAccent`,
/// otherwise `Primary`) which is paint-time data, not widget-construction
/// data.
pub struct CalendarDayConfig {
    /// Day-number string (e.g. `"15"`). The recipe builds the
    /// `TextWidget` itself so it can drive the text colour from
    /// `fill` (Selected → `OnAccent`, otherwise `Primary`). Accepts
    /// either a plain `String` or a `LocalizedString` from `tr!(…)` —
    /// `LocalizedString` implements `From<…> for String`,
    /// so `.into()` covers both shapes. Day numbers are pure digits
    /// in IntUI and pass through as untranslated literals, but a
    /// custom recipe is free to localize.
    pub label: String,
    /// Reactive selection-derived fill.
    pub fill: Signal<CalendarDayFill>,
    /// `true` when this cell's date equals the local "today". The
    /// recipe paints the today ring on top of any selection fill.
    pub is_today: bool,
    /// `true` when this cell's date falls outside the currently-visible
    /// month (leading / trailing 7-day padding). The recipe usually
    /// dims the label.
    pub is_out_of_month: bool,
    /// `true` when this cell is unselectable (filter, min/max, etc.).
    /// The recipe forces a disabled appearance regardless of fill.
    pub is_disabled: bool,
    /// `true` only while the parent calendar holds keyboard focus AND
    /// this cell's date is the currently-focused one (roving focus).
    pub is_focused_cell: Signal<bool>,
    /// Cell edge length — the recipe sizes its rect to this.
    pub cell_size: f32,
}

/// Per-cell input to `CalendarStyle::make_zoom_cell` (used for both
/// `MonthsGrid` and `YearsGrid` cells).
pub struct CalendarZoomCellConfig {
    /// Month-name / year-number string. The recipe builds the
    /// `TextWidget` itself so it can drive the text colour from
    /// `is_selected` (Selected → `OnAccent`, otherwise `Primary`).
    /// Month names ride in as `LocalizedString` from `tr!(…)`
    /// converted via `.into()`; year numbers ride in as untranslated
    /// literals.
    pub label: String,
    /// `true` when this cell represents the visible-month's month
    /// (Months grid) or year (Years grid).
    pub is_selected: Signal<bool>,
    /// Pointer-hover state (managed by the cell widget).
    pub is_hovered: Signal<bool>,
    /// Pointer-press state (managed by the cell widget).
    pub is_pressed: Signal<bool>,
    /// Cell footprint.
    pub cell_width: f32,
    pub cell_height: f32,
}

/// Pre-built header components passed to `make_header`. All five slots
/// are pre-built widgets; the recipe lays them out into a horizontal
/// strip. When `show_navigation = false` on the calendar, every arrow
/// slot is `None` and the recipe still returns a row containing just
/// the title.
pub struct CalendarHeaderConfig {
    /// Far-left "step coarser" arrow (« — prev year in Days mode, prev
    /// decade in Months mode, etc.).
    pub prev_double: Option<WidgetId>,
    /// Single-step prev arrow (‹).
    pub prev: Option<WidgetId>,
    /// Center title — a `Button` whose label reactively reflects the
    /// visible month / year / decade and whose `on_activate` demotes
    /// the calendar mode.
    pub title: WidgetId,
    /// Single-step next arrow (›).
    pub next: Option<WidgetId>,
    /// Far-right "step coarser" next arrow (»).
    pub next_double: Option<WidgetId>,
}

pub trait CalendarStyle: 'static {
    fn make_day_cell(&self, cfg: &CalendarDayConfig, ctx: &mut BuildContext) -> WidgetId;
    fn make_zoom_cell(&self, cfg: &CalendarZoomCellConfig, ctx: &mut BuildContext) -> WidgetId;
    fn make_header(&self, cfg: &CalendarHeaderConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedCalendarStyle = Rc<dyn CalendarStyle>;
