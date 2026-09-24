// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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
//!   in the visible month. Color comes from `BorderRole::Focused`,
//!   painted by the active `CalendarStyle::make_day_cell`.
//! - **Out-of-month cells** (the leading days from the previous month
//!   and trailing days from the next month that fill the 6×7 grid) are
//!   rendered with `TextRole::Disabled` and remain selectable (matching
//!   macOS / Material). To prevent selection use
//!   `disabled_date_filter`.
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
//! Every string below is in the user's language. The words come from the
//! framework's Fluent bundle, so an application that registers
//! `framework_locales()` gets them translated; every date inside them is
//! written by ICU for the widget tree's locale, in the locale's own order
//! and grammar and on the Gregorian calendar the grid is laid out in, never
//! assembled from numbers or from translated names (see
//! `common::datetime::written`).
//!
//! - Container: `Role::Grid`, named after the visible month
//!   ("Calendar, May 2026", "Calendrier, mai 2026"), and a polite live
//!   region. The AT-SPI, UIA and macOS adapters announce a live node when
//!   its *name* changes, so a change of month is announced on all three. The
//!   value is the day under the keyboard cursor in full, then the
//!   selection when there is one: "Saturday, May 2, 2026 (selected:
//!   Friday, May 1, 2026)". With the cursor on the one selected day, the
//!   day is said once: "Friday, May 1, 2026 (selected)". A range is joined
//!   by words ("… to …"), not an en-dash, which some screen readers skip.
//!   UIA and macOS raise a value-changed event on it; AT-SPI carries a
//!   string value on no interface, so the value never reaches an AT-SPI
//!   client at all.
//! - Header arrow buttons: `Role::Button` with localized labels
//!   ("Previous month", "Next month") and `Action::Click` advertised.
//! - Header month/year label: `Role::Button`, a `Ghost` `Button` whose
//!   activation demotes [`CalendarMode`] one level, swapping the body for
//!   the coarser grid in place; no popup is opened. Its name is its title:
//!   the month and year, the year, or the decade in words ("2020 to 2029").
//! - Weekday header row: `Role::Row` of `Role::ColumnHeader` cells, each
//!   labelled with the long weekday name (e.g. "Monday").
//! - Day cells: `Role::GridCell` named by the day in full
//!   ("Saturday, May 2, 2026", "samedi 2 mai 2026"), `set_selected`,
//!   `set_aria_current(Date)` on today, `set_disabled` for filter
//!   rejections, and `Action::Click` advertised. Keyboard focus roves on
//!   the Calendar root, so a cell never carries a focused flag.
//!
//! # Example
//!
//! ```ignore
//! use teksilo::widgets::{Calendar, common::datetime::Date};
//!
//! let date = ctx.signal(Some(Date::constant(2026, 5, 2)));
//! ctx.add(
//!     Calendar::single(date.clone())
//!         .show_today_button(true)
//!         .on_selection_changed(|d, ctx| ctx.send_intent(MyIntent::DateChanged(d))),
//! );
//! ```
//!
//! ## Touch and pen
//!
//! A day cell is a **tap target**, not a manipulator, and the distinction
//! decides what it declares. Its activation is `on_tap`, so it already lands on
//! the release; its 32 dp box already clears the WCAG 2.2 SC 2.5.8 floor at
//! Compact and follows the density ladder above it; and it produces no value
//! from the press position and owns no drag. It therefore does **not** declare
//! `touch_action(NONE)`: a finger that comes to rest on a day and then drags is
//! scrolling the dialog or form the calendar sits in, which is what a user
//! expects and what declaring NONE would forbid for nothing gained.

mod cell;
mod header;
#[cfg(test)]
mod tests;
mod zoom_grid;

use std::cell::RefCell;
use std::rc::Rc;
use teksilo_i18n::lit;

use jiff::civil::Weekday;
use teksilo_canvas::{Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accesskit::{Action, Live, Role};
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::widget::{EventContext, LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::{HandlerSet, WidgetBuilder};
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::resolve_message_widget;
use teksilo_tokens::{TextRole, TextStyleRole};

use crate::button::{Button, ButtonVariant};
use crate::common::datetime::Date;
use crate::common::datetime::types::{YearMonth, today_local, weekday_from_monday_zero};
use crate::common::datetime::weekday_short_key;
use crate::common::datetime::written::{date_locale, full_date, medium_date, month_and_year};
use crate::primitives::{Center, Divider, FixedSize, HStack, Padding, Spacer, TextWidget, VStack};
use crate::styles::recipe_calendar_style as cal_recipe;

use self::cell::DayCell;
use self::header::CalendarHeader;
use teksilo_i18n::{FluentValue, LanguageIdentifier, LocalizedString};

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
    /// 4×3 grid of years (current decade). Title shows "2020 to 2029".
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

/// Whether and how week numbers are displayed in the leading column of the day grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WeekNumberDisplay {
    /// No week-number column (default).
    #[default]
    None,
    /// ISO 8601 week number — week 1 is the week containing the first
    /// Thursday of the year. Adds a narrow column to the left of the day grid.
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
    label: Option<LocalizedString>,
    /// Enabled state, static or reactive. Forwarded to the arena at
    /// build time.
    enabled: Prop<bool>,
    on_selection_changed: Option<OnSelectionChanged>,
    on_range_changed: Option<OnRangeChanged>,
    on_month_changed: Option<OnMonthChanged>,
    on_activate: Option<OnActivate>,
    /// The locale every date the calendar writes is written in, resolved
    /// from the tree's locale in `build()`. Read again by `accessibility()`,
    /// which has no context of its own; a locale switch rebuilds the
    /// calendar (the binding sits at `Rebuild`), so it cannot go stale.
    lang: LanguageIdentifier,
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
            .field("enabled", &self.enabled.get())
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
            enabled: Prop::Static(true),
            on_selection_changed: None,
            on_range_changed: None,
            on_month_changed: None,
            on_activate: None,
            lang: date_locale(None),
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
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        self.label = Some(ls);
        self
    }

    /// Set the enabled state, statically or reactively. Forwarded to the
    /// arena at build time — a bound `Signal<bool>` updates live.
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
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
        // Global accessibility text scale: the calendar's cell/header sizes are
        // fixed constants read at build, so a scale change must *rebuild* (a
        // relayout won't recompute them). Bind the scale signal at `Rebuild`
        // level — exactly like `visible_month` — and multiply every dimension
        // constant by `scale` below. Rebuilding the Calendar reconstructs its
        // header / weekday row / body, so they all pick up the new scale.
        let scale = ctx.text_scale();
        ctx.text_scale_signal().bind_to(
            self_id,
            ctx.binding_registry(),
            teksilo_core::binding::BindingLevel::Rebuild,
        );
        // Forward the enabled state into the arena. After this point the
        // arena is the single source of truth.
        ctx.enabled_when(self_id, self.enabled.clone());
        // Inner cell/grid helpers still take an `enabled: bool`
        // snapshot which is fine for build-time decisions (they pass
        // it to the inner widgets which now consult the arena).
        let enabled = self.enabled.get();
        let week_numbers = self.week_numbers;
        let week_number_col_width = match week_numbers {
            WeekNumberDisplay::None => 0.0,
            _ => cal_recipe::CALENDAR_WEEK_NUMBER_COLUMN_WIDTH * scale,
        };

        // A locale switch must re-derive the first day of week: it is read from
        // `ctx.locale_signal()` at build time, and `WidgetTree::set_locale`
        // only calls `mark_all_dirty` (layout + paint), which never re-runs
        // `build()`. Without this binding the widget keeps rendering with
        // the pattern of whatever locale was active when it was first
        // built. Bound at `Rebuild` for the same reason `Calendar` binds
        // the text scale there — the value is a build-time constant, so a
        // relayout cannot pick it up.
        ctx.locale_signal().bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            teksilo_core::binding::BindingLevel::Rebuild,
        );

        // Resolve first day of week: explicit override → locale default → Monday.
        let tree_locale = ctx.locale_signal().get();
        let first_dow = self.first_day_of_week_override.unwrap_or_else(|| {
            crate::common::datetime::first_day_of_week_for_locale(
                tree_locale.as_deref().unwrap_or_default(),
            )
        });
        self.lang = date_locale(tree_locale.as_deref());

        // ── Header (prev / month-label / next) ──────────────────
        let header_id = if self.show_navigation {
            ctx.add(CalendarHeader::new(
                self.visible_month.clone(),
                self.focused_date.clone(),
                self.mode.clone(),
                self.on_month_changed.clone(),
                self.lang.clone(),
            ))
        } else {
            // Empty placeholder so layout shape stays consistent.
            ctx.add(FixedSize::new().width(0.0).height(0.0).child(Spacer::new()))
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
            lang: self.lang.clone(),
        });
        // Cell footprint for zoom modes derived from day grid cell
        // size so the body's overall width matches the day grid (7
        // day cells worth, divided across 3 zoom columns) and the
        // calendar's outer width stays constant across mode flips.
        let zoom_cell_height = (cal_recipe::CALENDAR_CELL_SIZE * 1.4).max(36.0) * scale;
        let zoom_cell_width = (cal_recipe::CALENDAR_CELL_SIZE * 7.0 / 3.0).max(64.0) * scale;
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
                    &self.lang,
                ))
            } else {
                None
            };

        // ── Assemble VStack ─────────────────────────────────────
        let mut col = VStack::new()
            .spacing(cal_recipe::CALENDAR_SECTION_GAP * scale)
            .child(header_id)
            .child(weekday_row_id)
            .child(grid_id);
        if let Some(footer_id) = footer_id {
            let divider_id = ctx.add(Divider::horizontal());
            col = col.child(divider_id).child(footer_id);
        }
        let col_id = ctx.add(col);
        let padded_id =
            ctx.add(Padding::uniform(cal_recipe::CALENDAR_OUTER_PADDING * scale).child(col_id));

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
                .background(teksilo_tokens::SurfaceRole::Raised)
                .border_color(teksilo_tokens::BorderRole::Default)
                .border_width(theme.shape.border_width)
                .corner_radius(teksilo_tokens::CornerRadius::uniform(
                    theme.shape.radius_popup,
                )),
        );
        // `Live::Off` on the one root everything else hangs from. The grid
        // is a polite live region so that its *name* speaks a change of
        // month (see `accessibility`), and `accesskit_consumer` hands a
        // node's politeness down to every descendant that sets none
        // (node.rs:906-910). The AT-SPI, UIA and macOS adapters announce
        // every named node that enters the filtered tree, or is renamed
        // there, with an inherited politeness other than off
        // (atspi_common adapter.rs:71-77 and node.rs:610-622, windows
        // adapter.rs:255-263 and 313-324, macos event.rs:236-241 and
        // 300-310). So once the day grid, the header and the zoom grids
        // were reachable, opening the calendar announced all 42 dates, the
        // weekdays and the header buttons, and a change of month announced
        // every renamed day beside the grid's name, in whatever order the
        // consumer's hash set gave. Orca speaks each announcement with
        // interrupt set (default.py `speakMessage`), so the last one won:
        // a date picked by the hash, not the month. The node stays a
        // `GenericContainer`, which every adapter drops with its children
        // promoted into the grid.
        let framed_id = ctx.add(
            crate::primitives::ZStack::new()
                .child(bg_id)
                .child(padded_id)
                .access_live(Live::Off),
        );
        self.root_child_id = Some(framed_id);

        // Keyboard handler attaches at the root so it covers the whole
        // calendar.
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
            teksilo_core::binding::BindingLevel::AccessibilityOnly,
        );
        self.focused_date.bind_to(
            self_id,
            registry,
            teksilo_core::binding::BindingLevel::AccessibilityOnly,
        );
        match &self.selection {
            SelectionBinding::Single(sig) => sig.bind_to(
                self_id,
                registry,
                teksilo_core::binding::BindingLevel::AccessibilityOnly,
            ),
            SelectionBinding::Range { value, .. } => value.bind_to(
                self_id,
                registry,
                teksilo_core::binding::BindingLevel::AccessibilityOnly,
            ),
        }

        vec![framed_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
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
            Some(s) => s.resolve_now(),
            None => resolve_message_widget(
                "calendar-name-with-month",
                &[("month", FluentValue::from(month_and_year(ym, &self.lang)))],
            ),
        };
        builder.set_name(label);

        // A polite live region. The AT-SPI, UIA and macOS adapters all
        // announce a live node's *name* when it changes, and only a label
        // takes its name from its value, so this speaks a change of month
        // (the name above). The cursor lives in the value below, which UIA
        // and macOS report as a value change on the focused grid instead.
        // The bindings registered in `build()` at `AccessibilityOnly`
        // re-run this on every change of month, cursor or selection.
        builder.set_live(Live::Polite);

        // Compose the value: keyboard focus first, then the committed
        // selection, every date in full. It is heard, not parsed: UIA and
        // macOS report its changes on the focused grid, which is how the day
        // under the cursor reaches a screen reader there. The words around
        // the dates are the framework's messages, and a range is joined by
        // words (`calendar-date-range`), not an en-dash some readers skip.
        let cursor = self.focused_date.get();
        let focused = full_date(cursor, &self.lang);
        let value_text = match &self.selection {
            SelectionBinding::Single(sig) => match sig.get() {
                // The cursor on the one selected day is that day, said once
                // and marked selected: "vendredi 12 mars 2027 (sélectionné)".
                // Written out as the cursor followed by the selection, it was
                // the same date twice, and that is where a single calendar
                // opens: a date field's popover puts its cursor on the date
                // the field holds. A range keeps both, since a day inside a
                // range is not the range.
                Some(day) if day == cursor => resolve_message_widget(
                    "calendar-value-on-selection",
                    &[("date", FluentValue::from(focused))],
                ),
                Some(day) => with_selection(focused, full_date(day, &self.lang)),
                None => focused,
            },
            SelectionBinding::Range { value, .. } => match value.get() {
                Some(r) => with_selection(
                    focused,
                    resolve_message_widget(
                        "calendar-date-range",
                        &[
                            ("start", FluentValue::from(full_date(r.start, &self.lang))),
                            ("end", FluentValue::from(full_date(r.end, &self.lang))),
                        ],
                    ),
                ),
                None => focused,
            },
        };
        builder.set_value(value_text);

        // Framework a11y walker sets `set_disabled` from arena state.
        builder.add_action(Action::Focus);
    }
}

/// The grid's value when something other than the day under the cursor is
/// selected: the cursor, then the selection, "samedi 13 mars 2027
/// (sélection : vendredi 12 mars 2027)".
fn with_selection(focused: String, selection: String) -> String {
    resolve_message_widget(
        "calendar-value-with-selection",
        &[
            ("focused", FluentValue::from(focused)),
            ("selection", FluentValue::from(selection)),
        ],
    )
}

// ── Internal builders ─────────────────────────────────────────────────

fn build_weekday_row(
    ctx: &mut BuildContext,
    first_dow: Weekday,
    week_number_col_width: f32,
) -> WidgetId {
    // `week_number_col_width` already carries the text scale (computed by the
    // caller). Apply the same scale to the local constants.
    let scale = ctx.text_scale();
    let mut row = HStack::new().spacing(cal_recipe::CALENDAR_CELL_GAP * scale);
    if week_number_col_width > 0.0 {
        // Empty corner cell above the week-number column.
        let spacer = ctx.add(
            FixedSize::new()
                .width(week_number_col_width)
                .height(cal_recipe::CALENDAR_WEEKDAY_ROW_HEIGHT * scale)
                .child(Spacer::new()),
        );
        row = row.child(spacer);
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
            cal_recipe::CALENDAR_CELL_SIZE * scale,
            cal_recipe::CALENDAR_WEEKDAY_ROW_HEIGHT * scale,
        );
        row = row.child(ctx.add(cell));
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
    /// The locale the day cells name their dates in.
    lang: LanguageIdentifier,
}

fn build_footer(
    ctx: &mut BuildContext,
    show_today: bool,
    visible_month: Signal<YearMonth>,
    focused_date: Signal<Date>,
    selection: SelectionBinding,
    on_selection_changed: Option<OnSelectionChanged>,
    on_month_changed: Option<OnMonthChanged>,
    lang: &LanguageIdentifier,
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
    if let SelectionBinding::Range { value, .. } = &selection {
        let status_label = TextWidget::new(lit!(""))
            .style(TextStyleRole::Body)
            .color(TextRole::Secondary)
            .text(range_status(value, lang))
            .single_line()
            .a11y_hidden();
        let spacer = ctx.add(Spacer::new());
        row = row.child(spacer).child(status_label);
    } else {
        row = row.child(Spacer::new());
    }
    ctx.add(row)
}

/// The line under a range calendar saying what is selected: "Sélection :
/// 1er mars 2027 – 12 mars 2027".
///
/// Derived from the committed range itself, so it follows a commit from the
/// keyboard or from the application as well as a click. It is on screen only
/// (the grid's value already says it, in words), so the shorter medium date
/// keeps it on one line and the en-dash costs nobody anything.
fn range_status(value: &Signal<Option<DateRange>>, lang: &LanguageIdentifier) -> Signal<String> {
    let lang = lang.clone();
    value.map(move |range| match range {
        Some(r) => resolve_message_widget(
            "calendar-range-status",
            &[
                ("start", FluentValue::from(medium_date(r.start, &lang))),
                ("end", FluentValue::from(medium_date(r.end, &lang))),
            ],
        ),
        None => String::new(),
    })
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
    ) -> teksilo_core::widget::LayoutResponse {
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

        // Grow the grid with the global accessibility text scale. A scale
        // change rebuilds the whole Calendar (the binding lives on the top-level
        // widget), so reading it at build and multiplying here is sufficient.
        let scale = ctx.text_scale();
        let mut row_ids = Vec::with_capacity(6);
        let cell_size = cal_recipe::CALENDAR_CELL_SIZE * scale;
        let cell_height = cal_recipe::CALENDAR_CELL_SIZE * scale;
        let gap = cal_recipe::CALENDAR_CELL_GAP * scale;
        let week_number_col_width = match self.params.week_numbers {
            WeekNumberDisplay::None => 0.0,
            _ => cal_recipe::CALENDAR_WEEK_NUMBER_COLUMN_WIDTH * scale,
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
                row = row.child(
                    ctx.add(
                        FixedSize::new()
                            .width(week_number_col_width)
                            .height(cell_height)
                            .child(Center::new().child(week_text_id)),
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
                    self.params.lang.clone(),
                );
                row = row.child(ctx.add(cell));
            }
            // AT: each week is a Role::Row; the WAI-ARIA grid pattern
            // expects Grid > Row > GridCell.
            row_ids.push(ctx.add(row.access_role(Role::Row)));
        }
        let mut col = VStack::new().spacing(gap);
        for id in &row_ids {
            col = col.child(*id);
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
            teksilo_core::binding::BindingLevel::Rebuild,
        );
        vec![col_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
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
        // Body itself is structural: the parent Calendar carries the
        // Role::Grid name. A bare `GenericContainer` keeps it from being
        // announced a second time, since every adapter drops it and
        // promotes the day cells. Never `set_hidden()`, which an adapter
        // reads as hiding the subtree: every day cell went with it.
        builder.set_role(Role::GenericContainer);
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
            // Accelerator + Home / End (⌘ on macOS) jumps to the first / last
            // day of the month; plain Home / End stay within the week.
            Key::Home if modifiers.command() => {
                let ym = YearMonth::from_date(cur);
                new_focus = Some(ym.first_day());
            }
            Key::End if modifiers.command() => {
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
