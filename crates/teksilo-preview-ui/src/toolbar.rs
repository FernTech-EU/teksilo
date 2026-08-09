// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Toolbar pane (top).
//!
//! Theme picker, background mode, and locale dropdown. Theme and
//! locale changes are applied via `EventContext::set_theme` /
//! `set_locale`, which require an event-time call; the toolbar renders
//! those two as rows of `Button`s so the active option can carry a
//! distinct variant. (`SegmentedControl::on_change` also hands over an
//! `EventContext` these days, so either shape would work.)
//!
//! Note: theme and locale apply tree-wide, not to the canvas
//! sub-tree alone — that requires per-subtree theme scoping which
//! the framework does not yet expose. For development purposes the
//! whole-app reskin is still useful (seeing a widget on dark mode
//! also reskins the chrome).
//!
//! Zoom is intentionally absent: real visual zoom requires a
//! transform-aware paint primitive (a `ScaleWidget` that pushes a
//! scaling transform onto the canvas) which teksilo-canvas does not
//! currently support. The zoom signal in `AppState` is preserved
//! for the future ScaleWidget to consume.

use teksilo_core::build_context::BuildContext;
use teksilo_core::widget::EventContext;
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::lit;
use teksilo_tokens::{BorderRole, SurfaceRole, TextRole, TextStyleRole};
use teksilo_widgets::primitives::{Padding, ZStack};
use teksilo_widgets::{
    Button, ButtonVariant, ComboBox, HStack, RectWidget, SegmentedControl, TextWidget,
};

use crate::app_state::{AppState, BackgroundMode, CanvasTheme};

pub fn build_toolbar(ctx: &mut BuildContext, state: &AppState) -> WidgetId {
    let theme_widget = build_theme_picker(ctx, state);
    let bg_widget = build_background_picker(ctx, state);
    let locale_widget = build_locale_picker(ctx, state);

    let row = HStack::new()
        .spacing(16.0)
        .child(labelled("Theme:", theme_widget))
        .child(labelled("Background:", bg_widget))
        .child(labelled("Locale:", locale_widget));

    let inner = ctx.add(Padding::symmetric(8.0, 12.0).child(row));
    let bg = ctx.add(
        RectWidget::new()
            .background(SurfaceRole::Raised)
            .border_color(BorderRole::Default)
            .border_width(1.0),
    );
    ctx.add(ZStack::new().add_child(bg).add_child(inner))
}

fn labelled(label: &str, control: WidgetId) -> impl teksilo_core::widget::Widget + 'static {
    let label_widget = TextWidget::new(lit!(label))
        .style(TextStyleRole::Tiny)
        .color(TextRole::Secondary)
        .single_line();
    HStack::new()
        .spacing(6.0)
        .child(label_widget)
        .add_child(control)
}

fn build_theme_picker(ctx: &mut BuildContext, state: &AppState) -> WidgetId {
    let mut row = HStack::new().spacing(4.0);
    for &theme_choice in CanvasTheme::ALL {
        let theme_sig = state.canvas_theme.clone();
        let current = state.canvas_theme.clone();
        let label = theme_choice.label();
        // Style the active option Default, others Flat. Reactive via
        // the theme signal so clicks restyle without a manual rebuild.
        let style_sig = current.map(move |t| {
            if *t == theme_choice {
                ButtonVariant::Filled
            } else {
                ButtonVariant::Ghost
            }
        });
        let style = style_sig.get();
        let btn = Button::new(lit!(label)).variant(style).on_activate_fn(
            move |ctx: &mut EventContext| {
                ctx.set_theme(theme_choice.theme());
                theme_sig.set(theme_choice);
            },
        );
        row = row.child(btn);
    }
    ctx.add(row)
}

fn build_background_picker(ctx: &mut BuildContext, state: &AppState) -> WidgetId {
    let labels: Vec<String> = BackgroundMode::ALL
        .iter()
        .map(|m| m.label().to_string())
        .collect();
    let initial_idx = BackgroundMode::ALL
        .iter()
        .position(|m| *m == state.background_mode.get())
        .unwrap_or(0);
    let idx_sig = ctx.signal(initial_idx);
    {
        let bg_sig = state.background_mode.clone();
        let h = idx_sig.observe(move |i| {
            if let Some(m) = BackgroundMode::ALL.get(*i)
                && bg_sig.get() != *m
            {
                bg_sig.set(*m);
            }
        });
        ctx.own_handle(h);
    }
    {
        let idx_sig = idx_sig.clone();
        let h = state.background_mode.observe(move |m| {
            if let Some(target) = BackgroundMode::ALL.iter().position(|x| x == m)
                && idx_sig.get() != target
            {
                idx_sig.set(target);
            }
        });
        ctx.own_handle(h);
    }
    ctx.add(SegmentedControl::indexed(idx_sig).segments(labels.into_iter().map(|s| lit!(s))))
}

fn build_locale_picker(ctx: &mut BuildContext, state: &AppState) -> WidgetId {
    // List of locales is fixed for v1 — `teksilo-i18n` ships en-US and
    // fr-FR. A real list would come from the registered I18nConfig.
    let locales: &[(&str, Option<&str>)] = &[
        ("Default", None),
        ("en-US", Some("en-US")),
        ("fr-FR", Some("fr-FR")),
    ];
    let mut row = HStack::new().spacing(4.0);
    for (label, locale_str) in locales {
        let locale_str_owned: Option<String> = locale_str.map(|s| s.to_string());
        let locale_sig = state.canvas_locale.clone();
        let current = state.canvas_locale.clone();
        let target = locale_str_owned.clone();
        let style_sig = current.map(move |s| {
            if s == &target {
                ButtonVariant::Filled
            } else {
                ButtonVariant::Ghost
            }
        });
        let style = style_sig.get();
        let btn = Button::new(lit!(*label)).variant(style).on_activate_fn(
            move |ctx: &mut EventContext| {
                if let Some(ref s) = locale_str_owned {
                    ctx.set_locale(s.clone());
                }
                locale_sig.set(locale_str_owned.clone());
            },
        );
        row = row.child(btn);
    }
    let _ = ComboBox::<String>::new(
        Vec::<String>::new(),
        teksilo_core::signal::Signal::new(None),
    );
    ctx.add(row)
}
