//! `Calendar` — month-grid date picker, standalone widget.
//!
//! A self-contained calendar with month/year navigation, a 6×7 day grid,
//! keyboard navigation matching the WAI-ARIA grid pattern, and full
//! AccessKit instrumentation (`Role::Grid` + per-cell `Role::GridCell`).
//! Used standalone for event apps and scheduling, and embedded in
//! [`DateEdit`](crate::date_edit::DateEdit)'s popover.
//!
//! # Selection modes
//!
//! - [`Calendar::single`] — pick one day. Bound to `Signal<Option<Date>>`.
//! - [`Calendar::range`] — pick a start + end day. Bound to
//!   `Signal<Option<DateRange>>`. Click first day → click second day to
//!   commit. Escape mid-selection cancels the in-progress anchor.
//!
//! # Behaviour
//!
//! - **Visible month** is independent of the selection — navigating past
//!   the selected month doesn't lose the selection.
//! - **Today highlight** draws a ring around today's cell whenever it's
//!   in the visible month. Color comes from `TextRole::Accent`.
//! - **Out-of-month cells** (the leading days from the previous month
//!   and trailing days from the next month that fill the 6×7 grid) are
//!   rendered with `TextRole::Disabled` and remain selectable (matching
//!   macOS / Material). To prevent selection use
//!   [`disabled_date_filter`](Self::disabled_date_filter).
//! - **Keyboard** (matches the WAI-ARIA `grid` pattern):
//!   - Arrow keys: move focus by one day.
//!   - Home / End: first / last day of week.
//!   - Ctrl+Home / Ctrl+End: first / last day of month.
//!   - PageUp / PageDown: previous / next month.
//!   - Shift+PageUp / Shift+PageDown: previous / next year.
//!   - Enter / Space: commit focused day to selection.
//!   - Escape: in range mode mid-selection, cancel anchor; otherwise
//!     bubble (popover hosts close).
//!   - `T`: jump focus to today.
//!
//! # Accessibility
//!
//! - Container — `Role::Grid` with `set_label("Calendar, May 2026")`
//!   (localized). Single-mode also sets `set_value` to the current ISO
//!   selection or empty; range mode sets `"start – end"`.
//! - Header arrow buttons — `Role::Button` with localized labels
//!   ("Previous month", "Next month") and `Action::Click` advertised.
//! - Header month/year label — `Role::Button` (clickable to open the
//!   month picker) with `set_has_popup(HasPopup::Grid)` and
//!   `set_expanded(open)`.
//! - Weekday header row — `Role::Row` of `Role::ColumnHeader` cells,
//!   each labelled with the long weekday name (e.g. "Monday").
//! - Day cells — `Role::GridCell` with localized long-form labels
//!   ("May 2, 2026"), `set_selected`, `set_focused`, `set_disabled` for
//!   filter rejections, and `Action::Click` advertised.
//!
//! # Example
//!
//! ```ignore
//! use bastyde::widgets::{Calendar, common::datetime::Date};
//!
//! let date = ctx.signal(Some(Date::constant(2026, 5, 2)));
//! ctx.add(
//!     Calendar::single(date.clone())
//!         .show_today_button(true)
//!         .on_selection_changed(|d, ctx| ctx.send_intent(MyIntent::DateChanged(d))),
//! );
//! ```

mod cell;
mod header;
#[cfg(test)]
mod tests;
mod zoom_grid;

use bastyde_i18n::lit;
use std::cell::RefCell;
use std::rc::Rc;

use bastyde_canvas::{Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::accesskit::{Action, Live, Role};
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::signal::Signal;
use bastyde_core::widget::{EventContext, LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::{HandlerSet, WidgetBuilder};
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::resolve_message_widget;
use bastyde_tokens::{TextRole, TextStyleRole};
use jiff::civil::Weekday;

use crate::button::{Button, ButtonVariant};
use crate::common::datetime::Date;
use crate::common::datetime::month_long_key;
use crate::common::datetime::types::{YearMonth, today_local, weekday_from_monday_zero};
use crate::common::datetime::weekday_short_key;
use crate::primitives::{Center, Divider, FixedSize, HStack, Padding, Spacer, TextWidget, VStack};
use crate::styles::recipe_calendar_style as cal_recipe;

use self::cell::DayCell;
use self::header::CalendarHeader;

// ── Public types ──────────────────────────────────────────────────────

/// Inclusive range of two dates, with `start <= end` enforced at
/// construction. Used by [`Calendar::range`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DateRange {
    pub start: Date,
    pub end: Date,
}

impl DateRange {
    /// Construct a range; swaps `start` and `end` if needed so the
    /// invariant `start <= end` always holds.
    pub fn new(a: Date, b: Date) -> Self {
        if a <= b {
            Self { start: a, end: b }
        } else {
            Self { start: b, end: a }
        }
    }

    /// `true` iff `d` is between `start` and `end` inclusive.
    pub fn contains(&self, d: Date) -> bool {
        d >= self.start && d <= self.end
    }
}

/// Selection mode discriminant — chosen at construction by picking
/// between [`Calendar::single`] and [`Calendar::range`]. Stored
/// internally; not part of the public surface.
#[derive(Clone)]
pub(crate) enum SelectionBinding {
    Single(Signal<Option<Date>>),
    Range {
        value: Signal<Option<DateRange>>,
        anchor: Signal<Option<Date>>,
    },
}

/// What the calendar body is showing — drives the WPF/Avalonia
/// "header-zoom" UX where clicking the title cycles to a coarser
/// grid, letting the user reach any year in 2-3 clicks instead of
/// many chevron presses. Default [`CalendarMode::Days`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CalendarMode {
    /// 6×7 day grid for the visible month. Title shows "May 2026".
    /// Header chevrons step by ±1 month and ±1 year.
    #[default]
    Days,
    /// 4×3 grid of months. Title shows "2026". Header chevrons step
    /// by ±1 year. Picking a cell zooms back into [`Self::Days`].
    Months,
    /// 4×3 grid of years (current decade). Title shows "2020 — 2029".
    /// Header chevrons step by ±10 years (one decade). Picking a cell
    /// zooms back into [`Self::Months`].
    Years,
}

impl CalendarMode {
    /// Mode after demoting one level (clicking the header title).
    /// `Years` is the coarsest level — no further demotion.
    pub fn demote(self) -> Self {
        match self {
            Self::Days => Self::Months,
            Self::Months => Self::Years,
            Self::Years => Self::Years,
        }
    }
}

/// Whether and how week numbers are displayed in the leading column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WeekNumberDisplay {
    /// No week-number column (default).
    #[default]
    None,
    /// ISO 8601 week number (week 1 is the week containing the first
    /// Thursday of the year).
    Iso8601,
}

// ── Builder API ───────────────────────────────────────────────────────

pub(crate) type DisabledDateFilter = Rc<dyn Fn(Date) -> bool>;
pub(crate) type OnSelectionChanged = Rc<dyn Fn(Option<Date>, &mut EventContext)>;
pub(crate) type OnRangeChanged = Rc<dyn Fn(Option<DateRange>, &mut EventContext)>;
pub(crate) type OnMonthChanged = Rc<dyn Fn(YearMonth, &mut EventContext)>;
pub(crate) type OnActivate = Rc<dyn Fn(Date, &mut EventContext)>;

/// Standalone month-grid date picker. See the [module docs](self) for
/// the full feature list and a usage example.
pub struct Calendar {
    selection: SelectionBinding,
    visible_month: Signal<YearMonth>,
    focused_date: Signal<Date>,
    /// Body mode (Days / Months / Years). Owned so the header label
    /// can demote it on click and the cells can promote it back
    /// (Years cell → Months → Days). Default [`CalendarMode::Days`].
    mode: Signal<CalendarMode>,
    /// Optional custom override of the locale-derived first day of week.
    first_day_of_week_override: Option<Weekday>,
    week_numbers: WeekNumberDisplay,
    show_today_button: bool,
    show_navigation: bool,
    min_date: Option<Date>,
    max_date: Option<Date>,
    disabled_date_filter: Option<DisabledDateFilter>,
    label: Option<String>,
    /// Initial enabled-state; forwarded to the arena at build time.
    initial_enabled: bool,
    on_selection_changed: Option<OnSelectionChanged>,
    on_range_changed: Option<OnRangeChanged>,
    on_month_changed: Option<OnMonthChanged>,
    on_activate: Option<OnActivate>,
    /// Status message shown at the bottom in range mode while a range
    /// is committed.
    range_status: Signal<String>,
    /// `true` while the Calendar root holds keyboard focus. Drives the
    /// roving-focus ring on the cell at `focused_date` so keyboard
    /// users see where the next arrow key will land. Written by
    /// `.on_focus()` in `build()`.
    focused: Signal<bool>,
    // Build state
    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for Calendar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Calendar")
            .field("initial_enabled", &self.initial_enabled)
            .finish_non_exhaustive()
    }
}

impl Calendar {
    /// Construct a calendar in single-selection mode bound to a
    /// nullable date signal.
    pub fn single(value: Signal<Option<Date>>) -> Self {
        let initial = value.get().unwrap_or_else(today_local);
        Self::new(SelectionBinding::Single(value), initial)
    }

    /// Construct a calendar in range-selection mode bound to a
    /// nullable date-range signal.
    pub fn range(value: Signal<Option<DateRange>>) -> Self {
        let initial = value.get().map(|r| r.start).unwrap_or_else(today_local);
        let anchor = Signal::new(None);
        Self::new(SelectionBinding::Range { value, anchor }, initial)
    }

    fn new(selection: SelectionBinding, initial_focus: Date) -> Self {
        Self {
            selection,
            visible_month: Signal::new(YearMonth::from_date(initial_focus)),
            focused_date: Signal::new(initial_focus),
            mode: Signal::new(CalendarMode::default()),
            first_day_of_week_override: None,
            week_numbers: WeekNumberDisplay::None,
            show_today_button: false,
            show_navigation: true,
            min_date: None,
            max_date: None,
            disabled_date_filter: None,
            label: None,
            initial_enabled: true,
            on_selection_changed: None,
            on_range_changed: None,
            on_month_changed: None,
            on_activate: None,
            range_status: Signal::new(String::new()),
            focused: Signal::new(false),
            root_child_id: None,
        }
    }

    /// Override the locale-derived first day of the week.
    pub fn first_day_of_week(mut self, w: Weekday) -> Self {
        self.first_day_of_week_override = Some(w);
        self
    }

    /// Show or hide the leading week-number column.
    pub fn week_numbers(mut self, mode: WeekNumberDisplay) -> Self {
        self.week_numbers = mode;
        self
    }

    /// Show a "Today" button in the footer that jumps focus and selection
    /// (in single mode) to today.
    pub fn show_today_button(mut self, show: bool) -> Self {
        self.show_today_button = show;
        self
    }

    /// Show or hide the prev/next month navigation arrows.
    pub fn show_navigation(mut self, show: bool) -> Self {
        self.show_navigation = show;
        self
    }

    /// Earliest allowed date; days before this read as disabled.
    pub fn min_date(mut self, d: Date) -> Self {
        self.min_date = Some(d);
        self
    }

    /// Latest allowed date; days after this read as disabled.
    pub fn max_date(mut self, d: Date) -> Self {
        self.max_date = Some(d);
        self
    }

    /// Per-cell predicate. `true` ⇒ cell is disabled (no click, no
    /// keyboard commit, AT marks `disabled`).
    pub fn disabled_date_filter(mut self, f: impl Fn(Date) -> bool + 'static) -> Self {
        self.disabled_date_filter = Some(Rc::new(f));
        self
    }

    /// Override the AT label. Default: "Calendar, May 2026" (localized,
    /// derived from the visible month).
    pub fn label(mut self, label: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        let ls: bastyde_i18n::LocalizedString = label.into();
        self.label = Some(ls.resolve_now());
        self
    }

    /// Set the initial enabled state. Forwarded to the arena at build
    /// time. Use `ctx.enabled_when(calendar_id, signal)` for reactivity.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.initial_enabled = enabled;
        self
    }

    /// Fired when the selection changes. In range mode use
    /// [`on_range_changed`](Self::on_range_changed) instead — this
    /// callback fires on every committed-day change in range mode too,
    /// passing the just-committed endpoint.
    pub fn on_selection_changed(
        mut self,
        f: impl Fn(Option<Date>, &mut EventContext) + 'static,
    ) -> Self {
        self.on_selection_changed = Some(Rc::new(f));
        self
    }

    /// Fired in range mode whenever a range is committed (second click
    /// of the pair). `None` fires when the user resets via Escape or
    /// when the bound value is externally cleared.
    pub fn on_range_changed(
        mut self,
        f: impl Fn(Option<DateRange>, &mut EventContext) + 'static,
    ) -> Self {
        self.on_range_changed = Some(Rc::new(f));
        self
    }

    /// Fired when the visible month changes (navigation arrows,
    /// keyboard PageUp/Down, today jump).
    pub fn on_month_changed(mut self, f: impl Fn(YearMonth, &mut EventContext) + 'static) -> Self {
        self.on_month_changed = Some(Rc::new(f));
        self
    }

    /// Fired in single mode on Enter or click (i.e. when the user
    /// "double commits"). Distinct from selection change; popover hosts
    /// use this to dismiss themselves only on a real click, not on
    /// keyboard navigation.
    pub fn on_activate(mut self, f: impl Fn(Date, &mut EventContext) + 'static) -> Self {
        self.on_activate = Some(Rc::new(f));
        self
    }

    /// Reactive accessor for the currently-visible month.
    pub fn visible_month_signal(&self) -> Signal<YearMonth> {
        self.visible_month.clone()
    }

    /// Reactive accessor for the focused-cell date.
    pub fn focused_date_signal(&self) -> Signal<Date> {
        self.focused_date.clone()
    }

    /// Reactive accessor for the body mode (Days / Months / Years).
    /// Drives the header-zoom UX. Apps can read this to react to mode
    /// changes, or write to it to programmatically zoom in/out.
    pub fn mode_signal(&self) -> Signal<CalendarMode> {
        self.mode.clone()
    }
}

impl Widget for Calendar {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme_signal().get();
        let self_id = ctx.self_id();
        // Forward initial-enabled into the arena; see IconButton.
        if !self.initial_enabled {
            ctx.enabled_when(self_id, false);
        }
        // Inner cell/grid helpers still take an `enabled: bool`
        // snapshot which is fine for build-time decisions (they pass
        // it to the inner widgets which now consult the arena).
        let enabled = self.initial_enabled;
        let week_numbers = self.week_numbers;
        let week_number_col_width = match week_numbers {
            WeekNumberDisplay::None => 0.0,
            _ => cal_recipe::CALENDAR_WEEK_NUMBER_COLUMN_WIDTH,
        };

        // Resolve first day of week: explicit override → locale default → Monday.
        let first_dow = self.first_day_of_week_override.unwrap_or_else(|| {
            let tag = ctx.locale_signal().get().unwrap_or_default();
            crate::common::datetime::first_day_of_week_for_locale(&tag)
        });

        // ── Header (prev / month-label / next) ──────────────────
        let header_id = if self.show_navigation {
            ctx.add(CalendarHeader::new(
                self.visible_month.clone(),
                self.focused_date.clone(),
                self.mode.clone(),
                self.on_month_changed.clone(),
            ))
        } else {
            // Empty placeholder so layout shape stays consistent.
            ctx.add(
                FixedSize::new()
                    .bind_width(0.0)
                    .bind_height(0.0)
                    .child(Spacer::new()),
            )
        };

        // ── Weekday header row ──────────────────────────────────
        // Only meaningful in Days mode; hidden in Months/Years zoom.
        let weekday_row_id = build_weekday_row(ctx, first_dow, week_number_col_width);
        ctx.visible_when(
            weekday_row_id,
            self.mode.map(|m| matches!(m, CalendarMode::Days)),
        );

        // ── Body Switcher: Days / Months / Years ────────────────
        // The mode signal drives a Switcher that mounts only the
        // currently-active body. Day grid keeps all its existing
        // wiring; the two zoom grids are minimal click-driven 4×3
        // pickers that promote the visible_month and zoom back in
        // when a cell is picked.
        let day_body = self::CalendarBody::new(BuildGridParams {
            visible_month: self.visible_month.clone(),
            focused_date: self.focused_date.clone(),
            focused: self.focused.clone(),
            selection: self.selection.clone(),
            first_dow,
            week_numbers,
            min_date: self.min_date,
            max_date: self.max_date,
            disabled_filter: self.disabled_date_filter.clone(),
            enabled,
            on_selection_changed: self.on_selection_changed.clone(),
            on_range_changed: self.on_range_changed.clone(),
            on_activate: self.on_activate.clone(),
            range_status: self.range_status.clone(),
        });
        // Cell footprint for zoom modes derived from day grid cell
        // size so the body's overall width matches the day grid (7
        // day cells worth, divided across 3 zoom columns) and the
        // calendar's outer width stays constant across mode flips.
        let zoom_cell_height = (cal_recipe::CALENDAR_CELL_SIZE * 1.4).max(36.0);
        let zoom_cell_width = (cal_recipe::CALENDAR_CELL_SIZE * 7.0 / 3.0).max(64.0);
        let months_body = zoom_grid::MonthsGrid::new(
            self.visible_month.clone(),
            self.mode.clone(),
            enabled,
            zoom_cell_width,
            zoom_cell_height,
        );
        let years_body = zoom_grid::YearsGrid::new(
            self.visible_month.clone(),
            self.mode.clone(),
            enabled,
            zoom_cell_width,
            zoom_cell_height,
        );
        let mode_index = self.mode.map(|m| match m {
            CalendarMode::Days => 0_usize,
            CalendarMode::Months => 1,
            CalendarMode::Years => 2,
        });
        let grid_id = ctx.add(
            crate::primitives::Switcher::new(mode_index)
                .child(day_body)
                .child(months_body)
                .child(years_body),
        );

        // ── Optional footer ─────────────────────────────────────
        let footer_id =
            if self.show_today_button || matches!(self.selection, SelectionBinding::Range { .. }) {
                Some(build_footer(
                    ctx,
                    self.show_today_button,
                    self.visible_month.clone(),
                    self.focused_date.clone(),
                    self.selection.clone(),
                    self.on_selection_changed.clone(),
                    self.on_month_changed.clone(),
                    self.range_status.clone(),
                    matches!(self.selection, SelectionBinding::Range { .. }),
                ))
            } else {
                None
            };

        // ── Assemble VStack ─────────────────────────────────────
        let mut col = VStack::new()
            .spacing(cal_recipe::CALENDAR_SECTION_GAP)
            .add_child(header_id)
            .add_child(weekday_row_id)
            .add_child(grid_id);
        if let Some(footer_id) = footer_id {
            let divider_id = ctx.add(Divider::horizontal());
            col = col.add_child(divider_id).add_child(footer_id);
        }
        let col_id = ctx.add(col);
        let padded_id =
            ctx.add(Padding::uniform(cal_recipe::CALENDAR_OUTER_PADDING).child_id(col_id));

        // Opaque background — Calendar can be used standalone (sits
        // on whatever surface the parent provides) or as a popover
        // overlay (anchored above arbitrary content). Without an
        // explicit surface fill, the popover-mode calendar bleeds
        // through to whatever's behind it. Use `SurfaceRole::Raised`
        // because popovers are conventionally raised one elevation
        // above the page surface; standalone usage on a `Panel`
        // looks the same since both `Main` and `Raised` resolve to
        // `surface_main` / `surface_raised` based on theme.
        let bg_id = ctx.add(
            crate::primitives::RectWidget::new()
                .background(bastyde_tokens::SurfaceRole::Raised)
                .border_color(bastyde_tokens::BorderRole::Default)
                .border_width(theme.shape.border_width)
                .corner_radius(bastyde_tokens::CornerRadius::uniform(
                    theme.shape.radius_popup,
                )),
        );
        let framed_id = ctx.add(
            crate::primitives::ZStack::new()
                .add_child(bg_id)
                .add_child(padded_id),
        );
        self.root_child_id = Some(framed_id);

        // Keyboard handler attaches at the root so it covers the whole
        // calendar. Preview-pass so arrow keys are consumed before any
        // descendant TextInputField sees them.
        // Single keyboard handler on `on_key` (not `on_key_preview`).
        // Bubble-pass routing covers both cases:
        //   * grid root focused → on_key fires on the calendar (target)
        //     → all keys handled, including Enter/Space → commit.
        //   * chevron / today button focused → button's on_key fires
        //     first; consumes Enter/Space (activates itself) and
        //     stops bubbling. For arrows / PageUp / etc. the button
        //     returns Ignored, so the event bubbles to the calendar
        //     and navigates cells.
        // This is the standard WAI-ARIA pattern: the focused widget
        // gets first crack at the key, and the grid catches what's
        // left. Using `on_key_preview` here breaks Enter/Space on
        // descendant buttons because preview is consume-or-not, with
        // no way to forward selectively.
        let key_handler = build_keyboard_handler(
            self.visible_month.clone(),
            self.focused_date.clone(),
            self.selection.clone(),
            self.min_date,
            self.max_date,
            self.disabled_date_filter.clone(),
            self.on_selection_changed.clone(),
            self.on_range_changed.clone(),
            self.on_activate.clone(),
            self.on_month_changed.clone(),
            enabled,
            first_dow,
        );

        // Track keyboard focus on the calendar root so cells can render
        // a roving-focus ring on the cell at `focused_date` only while
        // the calendar actually holds focus (Int UI behaviour: no
        // focus indicator on a non-focused control).
        let focused_signal = self.focused.clone();
        let handlers = HandlerSet::new()
            .focusable(enabled)
            .on_focus(move |has_focus, _ctx| {
                focused_signal.set(has_focus);
            })
            .on_key(key_handler);
        ctx.apply_self_handlers(handlers);

        // Bind reactive sources at AccessibilityOnly so the AT node's
        // `name` (visible_month → "Calendar, May 2026") and `value`
        // (focused_date + selection) refresh as the user navigates,
        // without forcing a layout/repaint.
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.visible_month.bind_to(
            self_id,
            registry,
            bastyde_core::binding::BindingLevel::AccessibilityOnly,
        );
        self.focused_date.bind_to(
            self_id,
            registry,
            bastyde_core::binding::BindingLevel::AccessibilityOnly,
        );
        match &self.selection {
            SelectionBinding::Single(sig) => sig.bind_to(
                self_id,
                registry,
                bastyde_core::binding::BindingLevel::AccessibilityOnly,
            ),
            SelectionBinding::Range { value, .. } => value.bind_to(
                self_id,
                registry,
                bastyde_core::binding::BindingLevel::AccessibilityOnly,
            ),
        }

        vec![framed_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        match self.root_child_id {
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
        self.root_child_id.into_iter().collect()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        let ym = self.visible_month.get();
        builder.set_role(Role::Grid);

        let label = match &self.label {
            Some(s) => s.clone(),
            None => {
                let month_name = resolve_message_widget(month_long_key(ym.month()), &[]);
                format!("Calendar, {} {}", month_name, ym.year())
            }
        };
        builder.set_name(label);

        // Live region: month-change AND roving-focus announcements
        // both propagate by mutating `set_value`. The framework
        // marks the node a11y-dirty when `visible_month` /
        // `focused_date` / `selection` change (bindings registered
        // in `build()` at `AccessibilityOnly` level), accessibility()
        // re-runs, and AT picks up the new value as a polite
        // announcement.
        builder.set_live(Live::Polite);

        // Compose the value: keyboard focus first (drives roving
        // focus announcements), then the committed selection. ASCII
        // " to " instead of an en-dash because some screen readers
        // skip U+2013.
        let focused = self.focused_date.get();
        let focused_str = format!(
            "{:04}-{:02}-{:02}",
            focused.year(),
            focused.month(),
            focused.day()
        );
        let selection_str = match &self.selection {
            SelectionBinding::Single(sig) => sig
                .get()
                .map(|d| format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day())),
            SelectionBinding::Range { value, .. } => value.get().map(|r| {
                format!(
                    "{:04}-{:02}-{:02} to {:04}-{:02}-{:02}",
                    r.start.year(),
                    r.start.month(),
                    r.start.day(),
                    r.end.year(),
                    r.end.month(),
                    r.end.day(),
                )
            }),
        };
        let value_text = match selection_str {
            Some(sel) => format!("{} (selected: {})", focused_str, sel),
            None => focused_str,
        };
        builder.set_value(value_text);

        // Framework a11y walker sets `set_disabled` from arena state.
        builder.add_action(Action::Focus);
    }
}

// ── Internal builders ─────────────────────────────────────────────────

fn build_weekday_row(
    ctx: &mut BuildContext,
    first_dow: Weekday,
    week_number_col_width: f32,
) -> WidgetId {
    let mut row = HStack::new().spacing(cal_recipe::CALENDAR_CELL_GAP);
    if week_number_col_width > 0.0 {
        // Empty corner cell above the week-number column.
        let spacer = ctx.add(
            FixedSize::new()
                .bind_width(week_number_col_width)
                .bind_height(cal_recipe::CALENDAR_WEEKDAY_ROW_HEIGHT)
                .child(Spacer::new()),
        );
        row = row.add_child(spacer);
    }
    let first_offset = first_dow.to_monday_zero_offset();
    for i in 0..7 {
        let dow = weekday_from_monday_zero(first_offset + i);
        let key = weekday_short_key(dow);
        let label = resolve_message_widget(key, &[]);
        let long_label =
            resolve_message_widget(crate::common::datetime::weekday_long_key(dow), &[]);
        let text = TextWidget::new(lit!(label))
            .style(TextStyleRole::Body)
            .color(TextRole::Secondary)
            .single_line()
            .a11y_hidden();
        let text_id = ctx.add(text);
        let cell = WeekdayHeaderCell::new(
            text_id,
            long_label,
            cal_recipe::CALENDAR_CELL_SIZE,
            cal_recipe::CALENDAR_WEEKDAY_ROW_HEIGHT,
        );
        row = row.add_child(ctx.add(cell));
    }
    // AT: the row containing the column headers is itself a Row.
    // WAI-ARIA grid pattern wants Row > ColumnHeader, not Group >
    // ColumnHeader.
    ctx.add(row.access_role(Role::Row))
}

struct BuildGridParams {
    visible_month: Signal<YearMonth>,
    focused_date: Signal<Date>,
    focused: Signal<bool>,
    selection: SelectionBinding,
    first_dow: Weekday,
    week_numbers: WeekNumberDisplay,
    min_date: Option<Date>,
    max_date: Option<Date>,
    disabled_filter: Option<DisabledDateFilter>,
    enabled: bool,
    on_selection_changed: Option<OnSelectionChanged>,
    on_range_changed: Option<OnRangeChanged>,
    on_activate: Option<OnActivate>,
    range_status: Signal<String>,
}

fn build_footer(
    ctx: &mut BuildContext,
    show_today: bool,
    visible_month: Signal<YearMonth>,
    focused_date: Signal<Date>,
    selection: SelectionBinding,
    on_selection_changed: Option<OnSelectionChanged>,
    on_month_changed: Option<OnMonthChanged>,
    range_status: Signal<String>,
    is_range_mode: bool,
) -> WidgetId {
    let mut row = HStack::new().spacing(8.0);
    if show_today {
        let today_label = resolve_message_widget("calendar-button-today", &[]);
        let cb_visible = visible_month.clone();
        let cb_focused = focused_date.clone();
        let cb_selection = selection.clone();
        let cb_on_sel = on_selection_changed.clone();
        let cb_on_month = on_month_changed.clone();
        let today_btn = Button::new(lit!(today_label))
            .variant(ButtonVariant::Filled)
            .on_activate_fn(move |ctx_evt| {
                let today = today_local();
                let new_month = YearMonth::from_date(today);
                if cb_visible.get() != new_month {
                    cb_visible.set(new_month);
                    if let Some(cb) = cb_on_month.as_ref() {
                        cb(new_month, ctx_evt);
                    }
                }
                cb_focused.set(today);
                if let SelectionBinding::Single(sig) = &cb_selection
                    && sig.get() != Some(today)
                {
                    sig.set(Some(today));
                    if let Some(cb) = cb_on_sel.as_ref() {
                        cb(Some(today), ctx_evt);
                    }
                }
                ctx_evt.request_frame();
            });
        row = row.child(today_btn);
    }
    if is_range_mode {
        let status_label = TextWidget::new(lit!(""))
            .style(TextStyleRole::Body)
            .color(TextRole::Secondary)
            .bind_text(range_status.clone())
            .single_line()
            .a11y_hidden();
        let spacer = ctx.add(Spacer::new());
        row = row.add_child(spacer).child(status_label);
    } else {
        row = row.child(Spacer::new());
    }
    ctx.add(row)
}

// ── Weekday header cell (per-cell a11y wrapper) ───────────────────────

#[derive(Debug)]
struct WeekdayHeaderCell {
    child_id: WidgetId,
    long_label: String,
    cell_size: f32,
    cell_height: f32,
}

impl WeekdayHeaderCell {
    fn new(child_id: WidgetId, long_label: String, cell_size: f32, cell_height: f32) -> Self {
        Self {
            child_id,
            long_label,
            cell_size,
            cell_height,
        }
    }
}

impl Widget for WeekdayHeaderCell {
    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        Size::new(self.cell_size, self.cell_height).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = Point::new(bounds.x, bounds.y);
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child_id]
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::ColumnHeader);
        builder.set_name(&self.long_label);
    }
}

// ── CalendarBody — the 6×7 grid widget ────────────────────────────────

struct CalendarBody {
    params: BuildGridParams,
    row_ids: RefCell<Vec<WidgetId>>,
}

impl std::fmt::Debug for CalendarBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CalendarBody").finish()
    }
}

impl CalendarBody {
    fn new(params: BuildGridParams) -> Self {
        Self {
            params,
            row_ids: RefCell::new(Vec::new()),
        }
    }
}

impl Widget for CalendarBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Compute the 6×7 grid once for the current visible month.
        let ym = self.params.visible_month.get();
        let first_of_month = ym.first_day();
        let first_dow_offset = first_of_month.weekday().to_monday_zero_offset();
        let target_first_offset = self.params.first_dow.to_monday_zero_offset();
        // Days to step backward from the first of the month to land on
        // the row's first day.
        let lead = (first_dow_offset - target_first_offset).rem_euclid(7);
        let grid_start = first_of_month
            .checked_sub(jiff::Span::new().days(lead as i32 as i64))
            .unwrap_or(first_of_month);

        let mut row_ids = Vec::with_capacity(6);
        let cell_size = cal_recipe::CALENDAR_CELL_SIZE;
        let cell_height = cal_recipe::CALENDAR_CELL_SIZE;
        let gap = cal_recipe::CALENDAR_CELL_GAP;
        let week_number_col_width = match self.params.week_numbers {
            WeekNumberDisplay::None => 0.0,
            _ => cal_recipe::CALENDAR_WEEK_NUMBER_COLUMN_WIDTH,
        };

        for week in 0..6 {
            let mut row = HStack::new().spacing(gap);
            if week_number_col_width > 0.0 {
                // ISO week number = week containing the Thursday.
                let week_first = grid_start
                    .checked_add(jiff::Span::new().days((week * 7) as i64))
                    .unwrap_or(grid_start);
                let iso_wk = week_first
                    .checked_add(jiff::Span::new().days(3i64))
                    .unwrap_or(week_first)
                    .iso_week_date();
                let label_text = format!("{}", iso_wk.week());
                let week_text = TextWidget::new(lit!(label_text))
                    .style(TextStyleRole::Body)
                    .color(TextRole::Secondary)
                    .single_line()
                    .a11y_hidden();
                let week_text_id = ctx.add(week_text);
                row = row.add_child(
                    ctx.add(
                        FixedSize::new()
                            .bind_width(week_number_col_width)
                            .bind_height(cell_height)
                            .child(Center::new().child_id(week_text_id)),
                    ),
                );
            }
            for day_idx in 0..7 {
                let day_offset = (week * 7 + day_idx) as i64;
                let day_date = grid_start
                    .checked_add(jiff::Span::new().days(day_offset))
                    .unwrap_or(grid_start);
                let cell = DayCell::new(
                    day_date,
                    self.params.visible_month.clone(),
                    self.params.focused_date.clone(),
                    self.params.focused.clone(),
                    self.params.selection.clone(),
                    cell_size,
                    self.params.min_date,
                    self.params.max_date,
                    self.params.disabled_filter.clone(),
                    self.params.enabled,
                    self.params.on_selection_changed.clone(),
                    self.params.on_range_changed.clone(),
                    self.params.on_activate.clone(),
                    self.params.range_status.clone(),
                );
                row = row.add_child(ctx.add(cell));
            }
            // AT: each week is a Role::Row; the WAI-ARIA grid pattern
            // expects Grid > Row > GridCell.
            row_ids.push(ctx.add(row.access_role(Role::Row)));
        }
        let mut col = VStack::new().spacing(gap);
        for id in &row_ids {
            col = col.add_child(*id);
        }
        let col_id = ctx.add(col);
        *self.row_ids.borrow_mut() = vec![col_id];
        // Bind `visible_month` at `Rebuild` level so navigating prev/
        // next month triggers a full re-`build()` of this widget,
        // regenerating the 42 DayCells with new dates. Relayout would
        // only re-measure existing cells, leaving them frozen on the
        // month they were constructed with.
        let self_id = ctx.self_id();
        self.params.visible_month.bind_to(
            self_id,
            ctx.binding_registry(),
            bastyde_core::binding::BindingLevel::Rebuild,
        );
        vec![col_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let row_ids = self.row_ids.borrow();
        match row_ids.first() {
            Some(id) => ctx
                .child_size(*id, proposal)
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
        self.row_ids.borrow().clone()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Body itself is structural — the parent Calendar carries the
        // Role::Grid name. Hide this from AT so screen readers don't
        // double-announce.
        builder.set_role(Role::Group);
        builder.set_hidden();
    }
}

// ── Keyboard handler factory ──────────────────────────────────────────

fn build_keyboard_handler(
    visible_month: Signal<YearMonth>,
    focused_date: Signal<Date>,
    selection: SelectionBinding,
    min_date: Option<Date>,
    max_date: Option<Date>,
    disabled_filter: Option<DisabledDateFilter>,
    on_selection_changed: Option<OnSelectionChanged>,
    on_range_changed: Option<OnRangeChanged>,
    on_activate: Option<OnActivate>,
    on_month_changed: Option<OnMonthChanged>,
    enabled: bool,
    first_dow: Weekday,
) -> impl Fn(&WidgetEvent, &mut EventContext) -> EventResponse + 'static {
    let first_offset = first_dow.to_monday_zero_offset();
    move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
        if !enabled {
            return EventResponse::Ignored;
        }
        let WidgetEvent::KeyDown { key, modifiers, .. } = event else {
            return EventResponse::Ignored;
        };
        let cur = focused_date.get();
        let mut new_focus: Option<Date> = None;
        let mut new_visible: Option<YearMonth> = None;
        let mut commit: bool = false;

        match key {
            Key::ArrowLeft => new_focus = step_focus(cur, -1),
            Key::ArrowRight => new_focus = step_focus(cur, 1),
            Key::ArrowUp => new_focus = step_focus(cur, -7),
            Key::ArrowDown => new_focus = step_focus(cur, 7),
            Key::Home if modifiers.ctrl() => {
                let ym = YearMonth::from_date(cur);
                new_focus = Some(ym.first_day());
            }
            Key::End if modifiers.ctrl() => {
                let ym = YearMonth::from_date(cur);
                new_focus = Some(ym.last_day());
            }
            Key::Home => {
                let dow_offset = cur.weekday().to_monday_zero_offset();
                let lead = (dow_offset - first_offset).rem_euclid(7);
                new_focus = step_focus(cur, -(lead as i32));
            }
            Key::End => {
                let dow_offset = cur.weekday().to_monday_zero_offset();
                let lead = (dow_offset - first_offset).rem_euclid(7);
                new_focus = step_focus(cur, 6 - lead as i32);
            }
            Key::PageUp if modifiers.shift() => {
                let ym = YearMonth::from_date(cur).offset_months(-12);
                new_visible = Some(ym);
                new_focus = clamp_to_month(cur, ym);
            }
            Key::PageDown if modifiers.shift() => {
                let ym = YearMonth::from_date(cur).offset_months(12);
                new_visible = Some(ym);
                new_focus = clamp_to_month(cur, ym);
            }
            Key::PageUp => {
                let ym = YearMonth::from_date(cur).offset_months(-1);
                new_visible = Some(ym);
                new_focus = clamp_to_month(cur, ym);
            }
            Key::PageDown => {
                let ym = YearMonth::from_date(cur).offset_months(1);
                new_visible = Some(ym);
                new_focus = clamp_to_month(cur, ym);
            }
            Key::Enter | Key::Space => {
                commit = true;
            }
            Key::Escape => {
                if let SelectionBinding::Range { anchor, .. } = &selection
                    && anchor.get().is_some()
                {
                    anchor.set(None);
                    return EventResponse::Handled;
                }
                return EventResponse::Ignored;
            }
            Key::Character(c) if (*c == 't' || *c == 'T') => {
                let today = today_local();
                let ym = YearMonth::from_date(today);
                if visible_month.get() != ym {
                    visible_month.set(ym);
                    if let Some(cb) = on_month_changed.as_ref() {
                        cb(ym, ctx);
                    }
                }
                focused_date.set(today);
                ctx.request_frame();
                return EventResponse::Handled;
            }
            _ => return EventResponse::Ignored,
        }

        if let Some(nf) = new_focus {
            // Clamp to min/max.
            let nf = match (min_date, max_date) {
                (Some(min), _) if nf < min => min,
                (_, Some(max)) if nf > max => max,
                _ => nf,
            };
            focused_date.set(nf);
            // If the new focus crosses out of the visible month, follow.
            let nfm = YearMonth::from_date(nf);
            if YearMonth::from_date(cur) != nfm && new_visible.is_none() {
                new_visible = Some(nfm);
            }
        }
        if let Some(nv) = new_visible
            && visible_month.get() != nv
        {
            visible_month.set(nv);
            if let Some(cb) = on_month_changed.as_ref() {
                cb(nv, ctx);
            }
        }
        if commit {
            let target = focused_date.get();
            if !is_date_disabled(target, min_date, max_date, disabled_filter.as_ref()) {
                commit_date(
                    target,
                    &selection,
                    on_selection_changed.as_ref(),
                    on_range_changed.as_ref(),
                    on_activate.as_ref(),
                    ctx,
                );
            }
        }
        ctx.request_frame();
        EventResponse::Handled
    }
}

fn step_focus(cur: Date, days: i32) -> Option<Date> {
    cur.checked_add(jiff::Span::new().days(days as i64)).ok()
}

fn clamp_to_month(cur: Date, ym: YearMonth) -> Option<Date> {
    let last = ym.last_day().day();
    let day = cur.day().min(last);
    Date::new(ym.year(), ym.month(), day).ok()
}

pub(crate) fn is_date_disabled(
    d: Date,
    min: Option<Date>,
    max: Option<Date>,
    filter: Option<&DisabledDateFilter>,
) -> bool {
    if let Some(min) = min
        && d < min
    {
        return true;
    }
    if let Some(max) = max
        && d > max
    {
        return true;
    }
    if let Some(f) = filter
        && f(d)
    {
        return true;
    }
    false
}

pub(crate) fn commit_date(
    d: Date,
    selection: &SelectionBinding,
    on_sel: Option<&OnSelectionChanged>,
    on_range: Option<&OnRangeChanged>,
    on_activate: Option<&OnActivate>,
    ctx: &mut EventContext,
) {
    match selection {
        SelectionBinding::Single(sig) => {
            sig.set(Some(d));
            if let Some(cb) = on_sel {
                cb(Some(d), ctx);
            }
            if let Some(cb) = on_activate {
                cb(d, ctx);
            }
        }
        SelectionBinding::Range { value, anchor } => {
            match anchor.get() {
                None => {
                    // First click: park the anchor; don't touch the
                    // committed `value` yet. Observers of `value`
                    // shouldn't see a transient one-day range.
                    // `on_selection_changed` fires to signal intent
                    // ("user clicked here, range pending"); the actual
                    // committed range arrives on the second click.
                    anchor.set(Some(d));
                    if let Some(cb) = on_sel {
                        cb(Some(d), ctx);
                    }
                }
                Some(start) => {
                    // Second click: build the range (DateRange::new
                    // swaps if end < start), drop the anchor, commit.
                    let range = DateRange::new(start, d);
                    anchor.set(None);
                    value.set(Some(range));
                    if let Some(cb) = on_range {
                        cb(Some(range), ctx);
                    }
                    if let Some(cb) = on_sel {
                        cb(Some(d), ctx);
                    }
                }
            }
        }
    }
}
