//! Calendar / DateEdit / TimeEdit / DateTimeEdit gallery.
//!
//! Run with: `cargo run -p datetime-pickers`.
//!
//! Demonstrates:
//! - Standalone `Calendar` in single-mode (graphical date picking).
//! - Range-mode `Calendar` with status footer.
//! - `DateEdit` (text + calendar popover).
//! - `TimeEdit` in 24h and 12h modes, with seconds toggle.
//! - `DateTimeEdit` (composite).

use fern_ui::core::WidgetPlacement;
use fern_ui::prelude::*;
use fern_ui::widgets::{
    Calendar, DateEdit, DateRange, DateRangeEdit, DateTimeEdit, GroupHeader, HStack, Padding,
    Panel, SecondsMode, TextWidget, TimeEdit, TimeFormat, VStack,
};
use jiff::civil::{Date, DateTime, Time};

#[derive(Debug)]
struct Root {
    selected_date: Signal<Option<Date>>,
    selected_range: Signal<Option<DateRange>>,
    edit_date: Signal<Option<Date>>,
    edit_time_24h: Signal<Option<Time>>,
    edit_time_12h: Signal<Option<Time>>,
    edit_dt: Signal<Option<DateTime>>,
    edit_range: Signal<Option<DateRange>>,
    root_child_id: Option<WidgetId>,
}

impl Root {
    fn new() -> Self {
        let initial_date = Date::constant(2026, 5, 2);
        let initial_time = Time::new(14, 35, 0, 0).unwrap();
        Self {
            selected_date: Signal::new(Some(initial_date)),
            selected_range: Signal::new(None),
            edit_date: Signal::new(Some(initial_date)),
            edit_time_24h: Signal::new(Some(initial_time)),
            edit_time_12h: Signal::new(Some(initial_time)),
            edit_dt: Signal::new(Some(initial_date.to_datetime(initial_time))),
            edit_range: Signal::new(Some(DateRange::new(
                initial_date,
                Date::constant(2026, 5, 16),
            ))),
            root_child_id: None,
        }
    }
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let single_value_text = self.selected_date.map(|d| match d {
            Some(d) => format!("Selected: {:04}-{:02}-{:02}", d.year(), d.month(), d.day()),
            None => "No date selected".to_string(),
        });
        let range_value_text = self.selected_range.map(|r| match r {
            Some(r) => format!(
                "Range: {:04}-{:02}-{:02} – {:04}-{:02}-{:02}",
                r.start.year(),
                r.start.month(),
                r.start.day(),
                r.end.year(),
                r.end.month(),
                r.end.day()
            ),
            None => "No range selected".to_string(),
        });
        let edit_date_text = self.edit_date.map(|d| match d {
            Some(d) => format!("Edit: {:04}-{:02}-{:02}", d.year(), d.month(), d.day()),
            None => "Edit: (empty)".to_string(),
        });
        let time24_text = self.edit_time_24h.map(|t| match t {
            Some(t) => format!("24h time: {:02}:{:02}", t.hour(), t.minute()),
            None => "24h time: (empty)".to_string(),
        });
        let time12_text = self.edit_time_12h.map(|t| match t {
            Some(t) => format!(
                "12h time: {:02}:{:02} {}",
                ((t.hour() + 11) % 12) + 1,
                t.minute(),
                if t.hour() < 12 { "AM" } else { "PM" }
            ),
            None => "12h time: (empty)".to_string(),
        });
        let dt_text = self.edit_dt.map(|dt| match dt {
            Some(dt) => format!(
                "DateTime: {:04}-{:02}-{:02} {:02}:{:02}",
                dt.date().year(),
                dt.date().month(),
                dt.date().day(),
                dt.time().hour(),
                dt.time().minute(),
            ),
            None => "DateTime: (empty)".to_string(),
        });

        // Section: Calendar (single)
        let calendar_single = Calendar::single(self.selected_date.clone()).show_today_button(true);
        let calendar_single_id = ctx.add(calendar_single);
        let single_status_id = ctx.add(
            TextWidget::new_literal("")
                .bind_text(single_value_text)
                .single_line(),
        );

        // Section: Calendar (range)
        let calendar_range = Calendar::range(self.selected_range.clone()).show_today_button(true);
        let calendar_range_id = ctx.add(calendar_range);
        let range_status_id = ctx.add(
            TextWidget::new_literal("")
                .bind_text(range_value_text)
                .single_line(),
        );

        // Section: DateEdit
        let date_edit = DateEdit::new(self.edit_date.clone())
            .min_date(Date::constant(2020, 1, 1))
            .max_date(Date::constant(2030, 12, 31))
            .placeholder("YYYY-MM-DD");
        let date_edit_id = ctx.add(date_edit);
        let date_edit_status_id = ctx.add(
            TextWidget::new_literal("")
                .bind_text(edit_date_text)
                .single_line(),
        );

        // Section: TimeEdit (24h)
        let time_24h = TimeEdit::new(self.edit_time_24h.clone()).format(TimeFormat::Hour24);
        let time_24h_id = ctx.add(time_24h);
        let time24_status_id = ctx.add(
            TextWidget::new_literal("")
                .bind_text(time24_text)
                .single_line(),
        );

        // Section: TimeEdit (12h with seconds)
        let time_12h = TimeEdit::new(self.edit_time_12h.clone())
            .format(TimeFormat::Hour12)
            .seconds(SecondsMode::Editable);
        let time_12h_id = ctx.add(time_12h);
        let time12_status_id = ctx.add(
            TextWidget::new_literal("")
                .bind_text(time12_text)
                .single_line(),
        );

        // Section: DateTimeEdit
        let dt_edit = DateTimeEdit::new(self.edit_dt.clone()).format_pattern_separator();
        let dt_edit_id = ctx.add(dt_edit);
        let dt_status_id = ctx.add(TextWidget::new_literal("").bind_text(dt_text).single_line());

        // Section: DateRangeEdit
        let range_edit_text = self.edit_range.map(|r| match r {
            Some(r) => format!(
                "Range edit: {:04}-{:02}-{:02} – {:04}-{:02}-{:02}",
                r.start.year(),
                r.start.month(),
                r.start.day(),
                r.end.year(),
                r.end.month(),
                r.end.day(),
            ),
            None => "Range edit: (empty)".to_string(),
        });
        let range_edit = DateRangeEdit::new(self.edit_range.clone())
            .min_date(Date::constant(2020, 1, 1))
            .max_date(Date::constant(2030, 12, 31))
            .placeholder_start("Start")
            .placeholder_end("End");
        let range_edit_id = ctx.add(range_edit);
        let range_edit_status_id = ctx.add(
            TextWidget::new_literal("")
                .bind_text(range_edit_text)
                .single_line(),
        );

        // Assemble two columns.
        let cols = HStack::new()
            .spacing(24.0)
            .child(
                Panel::new().child(
                    Padding::uniform(12.0).child(
                        VStack::new()
                            .spacing(8.0)
                            .child(GroupHeader::new_literal("Calendar (single)"))
                            .add_child(calendar_single_id)
                            .add_child(single_status_id),
                    ),
                ),
            )
            .child(
                Panel::new().child(
                    Padding::uniform(12.0).child(
                        VStack::new()
                            .spacing(8.0)
                            .child(GroupHeader::new_literal("Calendar (range)"))
                            .add_child(calendar_range_id)
                            .add_child(range_status_id),
                    ),
                ),
            );

        let editors = Panel::new().child(
            Padding::uniform(12.0).child(
                VStack::new()
                    .spacing(10.0)
                    .child(GroupHeader::new_literal("Inline editors"))
                    .child(
                        HStack::new()
                            .spacing(12.0)
                            .add_child(date_edit_id)
                            .add_child(date_edit_status_id),
                    )
                    .child(
                        HStack::new()
                            .spacing(12.0)
                            .add_child(time_24h_id)
                            .add_child(time24_status_id),
                    )
                    .child(
                        HStack::new()
                            .spacing(12.0)
                            .add_child(time_12h_id)
                            .add_child(time12_status_id),
                    )
                    .child(
                        HStack::new()
                            .spacing(12.0)
                            .add_child(dt_edit_id)
                            .add_child(dt_status_id),
                    )
                    .child(
                        HStack::new()
                            .spacing(12.0)
                            .add_child(range_edit_id)
                            .add_child(range_edit_status_id),
                    ),
            ),
        );

        let root_widget =
            Padding::uniform(16.0).child(VStack::new().spacing(16.0).child(cols).child(editors));
        let root_id = ctx.add(root_widget);
        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
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
}

// Tiny helper trait to avoid pulling in additional layout primitives.
trait DateTimeEditCompat {
    fn format_pattern_separator(self) -> Self;
}

impl DateTimeEditCompat for DateTimeEdit {
    fn format_pattern_separator(self) -> Self {
        self.separator(" — ")
    }
}

fn main() {
    // Register the framework's i18n bundle so a11y labels, weekday
    // names ("Monday"), month names ("May"), and the calendar's
    // "Today" / "Previous month" / "Next month" buttons render as
    // text rather than as raw Fluent keys (e.g.
    // `calendar-month-long-may`). Without this registration the
    // resolver falls back to the key itself — visible in the UI as
    // the literal key string.
    let i18n_config = fern_ui::i18n::I18nConfig::new()
        .source_locale("en-US".parse().unwrap())
        .supported_locales(["en-US".parse().unwrap(), "fr-FR".parse().unwrap()])
        .auto_detect_os_locale(true)
        .fallback_locale("en-US".parse().unwrap())
        .framework_locales(fern_ui::widgets::framework_locales());

    FernAppBuilder::new()
        .theme(Theme::light_default())
        .i18n(i18n_config)
        .initial_window(
            WindowConfig::new()
                .title("FernUI — Date / Time pickers")
                .size(960, 720)
                .root(|tree, _state| tree.add(Root::new())),
        )
        .run();
}
