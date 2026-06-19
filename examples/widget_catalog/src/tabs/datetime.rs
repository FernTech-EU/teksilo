// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Date & Time tab — Calendar, DateEdit, TimeEdit, DateTimeEdit, DateRangeEdit.

use bastyde::prelude::*;
use bastyde::widgets::{
    Calendar, DateEdit, DateRange, DateRangeEdit, DateTimeEdit, Divider, FixedSize, MaxSize,
    TextWidget, TimeEdit, VStack,
};
use jiff::civil::{Date, DateTime, Time};

use crate::shared::{Signals, section, tab_header};

pub fn title() -> LocalizedString {
    tr!(tab_datetime_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_datetime_refs())
}

pub fn classic(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());
    let cal_single_signal: Signal<Option<Date>> = ctx.signal(None);
    let cal_range_signal: Signal<Option<DateRange>> = ctx.signal(None);
    let date_signal: Signal<Option<Date>> = ctx.signal(None);
    let time_signal: Signal<Option<Time>> = ctx.signal(None);
    let dt_signal: Signal<Option<DateTime>> = ctx.signal(None);
    let range_signal: Signal<Option<DateRange>> = ctx.signal(None);

    let calendar_single = section(
        ctx,
        tr!(dt_calendar_single()),
        FixedSize::new()
            .bind_width(280.0_f32)
            .child(Calendar::single(cal_single_signal).show_today_button(true)),
    );
    let calendar_range = section(
        ctx,
        tr!(dt_calendar_range()),
        FixedSize::new()
            .bind_width(280.0_f32)
            .child(Calendar::range(cal_range_signal).show_today_button(true)),
    );
    // Each edit is capped near its own content width — `MaxSize` makes a
    // flex field fill the cap, so the free-text 360 dp width would read as
    // far too wide for "YYYY-MM-DD" / "HH:MM".
    let date_edit = section(
        ctx,
        lit!("DateEdit"),
        MaxSize::width(180.0).child(DateEdit::new(date_signal)),
    );
    let time_edit = section(
        ctx,
        lit!("TimeEdit"),
        MaxSize::width(120.0).child(TimeEdit::new(time_signal)),
    );
    let datetime_edit = section(
        ctx,
        lit!("DateTimeEdit"),
        MaxSize::width(290.0).child(DateTimeEdit::new(dt_signal)),
    );
    let date_range_edit = section(
        ctx,
        lit!("DateRangeEdit"),
        MaxSize::width(340.0).child(DateRangeEdit::new(range_signal)),
    );

    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(calendar_single)
            .add_child(calendar_range)
            .add_child(date_edit)
            .add_child(time_edit)
            .add_child(datetime_edit)
            .add_child(date_range_edit),
    )
}

pub fn bati(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let cal_single_signal: Signal<Option<Date>> = ctx.signal(None);
    let cal_range_signal: Signal<Option<DateRange>> = ctx.signal(None);
    let date_signal: Signal<Option<Date>> = ctx.signal(None);
    let time_signal: Signal<Option<Time>> = ctx.signal(None);
    let dt_signal: Signal<Option<DateTime>> = ctx.signal(None);
    let range_signal: Signal<Option<DateRange>> = ctx.signal(None);

    bati!(ctx => VStack {
            spacing: 20.0
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_datetime_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_datetime_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(dt_calendar_single())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    bind_width: 280.0_f32
                    Calendar::single(cal_single_signal) {
                        show_today_button: true
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(dt_calendar_range())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    bind_width: 280.0_f32
                    Calendar::range(cal_range_signal) {
                        show_today_button: true
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("DateEdit")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                MaxSize::width(180.0) {
                    DateEdit::new(date_signal)
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("TimeEdit")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                MaxSize::width(120.0) {
                    TimeEdit::new(time_signal)
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("DateTimeEdit")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                MaxSize::width(290.0) {
                    DateTimeEdit::new(dt_signal)
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("DateRangeEdit")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                MaxSize::width(340.0) {
                    DateRangeEdit::new(range_signal)
                }
            }
        }
    )
}
