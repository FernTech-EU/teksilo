//! Default `CalendarStyle` impl driven by paint-recipe data.
//!
//! `RecipeCalendarStyle` ports the IntUI calendar chrome exactly:
//!
//! * **Day cell** — the per-state background fill (Selected → accent;
//!   InRange → `SelectedInactive`; otherwise transparent), an optional
//!   today-ring border, the reactive roving-focus ring, and a centered
//!   day-number label whose colour follows the fill state.
//! * **Zoom cell** — selected/pressed/hover/transparent background
//!   precedence + a centered month/year label whose colour flips to
//!   `OnAccent` when selected.
//! * **Header** — the 5-slot row: prev-double, prev, title (Expand'd to
//!   fill), next, next-double, with a small inter-button gap.
//!
//! Calendar-specific layout numbers (cell size / gap, header height,
//! ring widths, etc.) live as `pub const`s on this module. `calendar.rs`
//! reads them directly when it needs sizing data outside the cell chrome
//! (weekday row, week-number column, outer padding, mode-switcher
//! footprint).

use bastyde_core::build_context::BuildContext;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{
    CalendarDayConfig, CalendarDayFill, CalendarHeaderConfig, CalendarStyle, CalendarZoomCellConfig,
};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{BorderRole, CornerRadius, SurfaceRole, TextRole, TextStyleRole};

use crate::primitives::{Center, Expand, FixedSize, HStack, RectWidget, TextWidget, ZStack};

// ─── IntUI design tokens for Calendar ──────────────────────────────
// `calendar.rs` reads these directly for sizing outside the per-cell
// chrome (weekday row, outer padding, footer divider, mode-switcher
// footprint).

/// Outer padding inside the calendar's framed surface.
pub const CALENDAR_OUTER_PADDING: f32 = 8.0;
/// Vertical gap between header / weekday row / day grid / footer.
pub const CALENDAR_SECTION_GAP: f32 = 4.0;
/// Height of the navigation header row (prev / label / next).
pub const CALENDAR_HEADER_HEIGHT: f32 = 28.0;
/// Height of the weekday-name row.
pub const CALENDAR_WEEKDAY_ROW_HEIGHT: f32 = 20.0;
/// Side length of each day cell (square cells).
pub const CALENDAR_CELL_SIZE: f32 = 32.0;
/// Visible day-cell content radius (selection fill).
pub const CALENDAR_CELL_RADIUS: f32 = 4.0;
/// Gap between day cells in both axes.
pub const CALENDAR_CELL_GAP: f32 = 0.0;
/// Stroke width of the today ring.
pub const CALENDAR_TODAY_RING_WIDTH: f32 = 1.0;
/// Edge length of header navigation arrow icons.
pub const CALENDAR_NAV_ICON_SIZE: f32 = 12.0;
/// Width of the optional week-number column.
pub const CALENDAR_WEEK_NUMBER_COLUMN_WIDTH: f32 = 28.0;
/// Header nav-arrow button footprint.
pub const CALENDAR_NAV_ARROW_SIZE: f32 = 24.0;
/// Header nav-arrow corner radius.
pub const CALENDAR_NAV_ARROW_RADIUS: f32 = 4.0;
/// Horizontal gap between the five header buttons.
pub const CALENDAR_HEADER_GAP: f32 = 4.0;
/// Cell corner radius shared by `MonthsGrid` and `YearsGrid` zoom cells.
pub const CALENDAR_ZOOM_CELL_RADIUS: f32 = 6.0;

/// Default `CalendarStyle` shipped with Bastyde.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeCalendarStyle;

impl CalendarStyle for RecipeCalendarStyle {
    fn make_day_cell(&self, cfg: &CalendarDayConfig, ctx: &mut BuildContext) -> WidgetId {
        let radius = CALENDAR_CELL_RADIUS;
        let today_ring_width = CALENDAR_TODAY_RING_WIDTH;

        // ── Background fill — Selected → Selected, InRange →
        // SelectedInactive, otherwise Transparent.
        let bg_role: Signal<SurfaceRole> = cfg.fill.map(|f| match f {
            CalendarDayFill::Selected => SurfaceRole::Selected,
            CalendarDayFill::InRange => SurfaceRole::SelectedInactive,
            CalendarDayFill::None => SurfaceRole::Transparent,
        });
        let bg_widget = RectWidget::new()
            .bind_background(bg_role)
            .corner_radius(CornerRadius::uniform(radius));
        let bg_id = ctx.add(bg_widget);

        // ── Today ring — outline border drawn on top of the bg.
        let ring_id = if cfg.is_today && !cfg.is_out_of_month {
            let ring = RectWidget::new()
                .background(SurfaceRole::Transparent)
                .border_color(BorderRole::Focused)
                .border_width(today_ring_width)
                .corner_radius(CornerRadius::uniform(radius));
            Some(ctx.add(ring))
        } else {
            None
        };

        // ── Roving focus ring — visible only while the parent
        // Calendar holds keyboard focus AND this cell's date is the
        // currently-focused one.
        let focus_ring_width = ctx.theme_signal().get().shape.focus_ring_width;
        let focus_ring = RectWidget::new()
            .background(SurfaceRole::Transparent)
            .border_color(BorderRole::Focused)
            .border_width(focus_ring_width)
            .corner_radius(CornerRadius::uniform(radius));
        let focus_ring_id = ctx.add(focus_ring);
        ctx.visible_when(focus_ring_id, cfg.is_focused_cell.clone());

        // ── Day-number label. Text colour: Disabled when disabled
        // or out-of-month; otherwise Selected → OnAccent / Normal →
        // Primary tracked via fill.
        let text_color: ColorProp = if cfg.is_disabled || cfg.is_out_of_month {
            TextRole::Disabled.into()
        } else {
            let role_signal: Signal<TextRole> = cfg.fill.map(|f| match f {
                CalendarDayFill::Selected => TextRole::OnAccent,
                _ => TextRole::Primary,
            });
            ColorProp::DynamicTextRole(role_signal)
        };
        let label = TextWidget::new_literal(cfg.label.clone())
            .style(TextStyleRole::Body)
            .bind_color(text_color)
            .single_line()
            .a11y_hidden();
        let label_id = ctx.add(label);
        let centered = ctx.add(Center::new().child_id(label_id));

        // ── Compose: ZStack[bg, today_ring?, focus_ring, label]
        let mut z = ZStack::new().add_child(bg_id);
        if let Some(ring) = ring_id {
            z = z.add_child(ring);
        }
        z = z.add_child(focus_ring_id).add_child(centered);
        let z_id = ctx.add(z);

        ctx.add(
            FixedSize::new()
                .bind_width(cfg.cell_size)
                .bind_height(cfg.cell_size)
                .child_id(z_id),
        )
    }

    fn make_zoom_cell(&self, cfg: &CalendarZoomCellConfig, ctx: &mut BuildContext) -> WidgetId {
        // Background role precedence: Selected → Pressed → Hover →
        // Transparent. Same precedence the day grid uses.
        let bg_role = cfg
            .is_selected
            .clone()
            .zip3(&cfg.is_hovered, &cfg.is_pressed)
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
        let text_role = cfg.is_selected.map(|sel| {
            if *sel {
                TextRole::OnAccent
            } else {
                TextRole::Primary
            }
        });

        let bg = RectWidget::new()
            .background(ColorProp::DynamicSurfaceRole(bg_role))
            .corner_radius(CornerRadius::uniform(CALENDAR_ZOOM_CELL_RADIUS));
        let bg_id = ctx.add(bg);

        let text = TextWidget::new_literal(cfg.label.clone())
            .style(TextStyleRole::Body)
            .color(ColorProp::DynamicTextRole(text_role))
            .single_line();
        let text_id = ctx.add(Center::new().child(text));

        let z_id = ctx.add(ZStack::new().add_child(bg_id).add_child(text_id));
        ctx.add(
            FixedSize::new()
                .bind_width(cfg.cell_width)
                .bind_height(cfg.cell_height)
                .child_id(z_id),
        )
    }

    fn make_header(&self, cfg: &CalendarHeaderConfig, ctx: &mut BuildContext) -> WidgetId {
        // Wrap the title in an `Expand::horizontal()` so it fills the
        // slack between the leading and trailing arrow pairs — the
        // pre-migration layout used the same trick.
        let title_filled = ctx.add(Expand::horizontal().child_id(cfg.title));

        let mut row = HStack::new().spacing(CALENDAR_HEADER_GAP);
        if let Some(id) = cfg.prev_double {
            row = row.add_child(id);
        }
        if let Some(id) = cfg.prev {
            row = row.add_child(id);
        }
        row = row.add_child(title_filled);
        if let Some(id) = cfg.next {
            row = row.add_child(id);
        }
        if let Some(id) = cfg.next_double {
            row = row.add_child(id);
        }
        ctx.add(row)
    }
}
