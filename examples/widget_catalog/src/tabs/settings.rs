// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Settings tab — ShortcutSettings + live PrivacySettings (telemetry consent UI).

use teksilo::prelude::*;
use teksilo::widgets::{
    Divider, Expand, LanguageSwitcher, Panel, PrivacySettings, ShortcutSettings, TextScaleControl,
    TextWidget, ThemeSwitcher, VStack,
};

use crate::shared::{Signals, section, tab_header};

pub fn title() -> LocalizedString {
    tr!(tab_settings_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_settings_refs())
}

pub fn classic(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());
    // App-wide preference widgets, each showcased in its own named section
    // (like the other widgets on this tab). `TextScaleControl` binds to the
    // persisted text-scale setting (reachable here since `ctx.settings()` is
    // live).
    let scale = ctx
        .settings()
        .signal_for(&teksilo::settings::TEXT_SCALE_KEY);
    let theme_switcher = section(
        ctx,
        lit!("ThemeSwitcher"),
        Panel::new()
            .background(SurfaceRole::Raised)
            .border_color(BorderRole::Default)
            .border_width(1.0)
            .corner_radius(8.0)
            .padding(16.0)
            .child(ThemeSwitcher::new()),
    );
    let text_scale = section(
        ctx,
        lit!("TextScaleControl"),
        Panel::new()
            .background(SurfaceRole::Raised)
            .border_color(BorderRole::Default)
            .border_width(1.0)
            .corner_radius(8.0)
            .padding(16.0)
            .child(TextScaleControl::new(scale).label(lit!("Text size"))),
    );
    let language_switcher = section(
        ctx,
        lit!("LanguageSwitcher"),
        Panel::new()
            .background(SurfaceRole::Raised)
            .border_color(BorderRole::Default)
            .border_width(1.0)
            .corner_radius(8.0)
            .padding(16.0)
            .child(LanguageSwitcher::new()),
    );
    // Wrap in a Panel for visual delineation, and an
    // `Expand::horizontal()` so the widget claims the tab's full width
    // (without it, ShortcutSettings reports its natural row-content
    // width and hugs the leading edge). Height is unbounded — the
    // catalog's TabContent already scrolls vertically.
    let shortcuts = section(
        ctx,
        lit!("ShortcutSettings"),
        Panel::new()
            .background(SurfaceRole::Raised)
            .border_color(BorderRole::Default)
            .border_width(1.0)
            .corner_radius(8.0)
            .padding(16.0)
            .child(
                Expand::horizontal()
                    .respect_intrinsic()
                    .child(ShortcutSettings::new()),
            ),
    );
    // Live consent UI. A no-network StubReporter is wired in main.rs so
    // this renders the real scope toggles / accept-reject / inspect
    // accordion rather than the "telemetry not configured" placeholder.
    let privacy = section(
        ctx,
        lit!("PrivacySettings"),
        Panel::new()
            .background(SurfaceRole::Raised)
            .border_color(BorderRole::Default)
            .border_width(1.0)
            .corner_radius(8.0)
            .padding(16.0)
            .child(
                Expand::horizontal()
                    .respect_intrinsic()
                    .child(PrivacySettings::new()),
            ),
    );
    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(theme_switcher)
            .add_child(text_scale)
            .add_child(language_switcher)
            .add_child(shortcuts)
            .add_child(privacy),
    )
}

pub fn teksu(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    // teksu! cannot express the parameterless `Expand::respect_intrinsic()`
    // builder method as a property — pre-build in Rust, drop the id in.
    let scale = ctx
        .settings()
        .signal_for(&teksilo::settings::TEXT_SCALE_KEY);
    let theme_switcher_body_id = ctx.add(ThemeSwitcher::new());
    let text_scale_body_id = ctx.add(TextScaleControl::new(scale).label(lit!("Text size")));
    let language_switcher_body_id = ctx.add(LanguageSwitcher::new());
    let shortcut_body_id = ctx.add(
        Expand::horizontal()
            .respect_intrinsic()
            .child(ShortcutSettings::new()),
    );
    let privacy_body_id = ctx.add(
        Expand::horizontal()
            .respect_intrinsic()
            .child(PrivacySettings::new()),
    );

    teksu!(ctx => VStack {
            spacing: 20.0
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_settings_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_settings_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("ThemeSwitcher")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Panel {
                    background: SurfaceRole::Raised
                    border_color: BorderRole::Default
                    border_width: 1.0
                    corner_radius: 8.0
                    padding: 16.0
                    child_id: theme_switcher_body_id
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("TextScaleControl")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Panel {
                    background: SurfaceRole::Raised
                    border_color: BorderRole::Default
                    border_width: 1.0
                    corner_radius: 8.0
                    padding: 16.0
                    child_id: text_scale_body_id
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("LanguageSwitcher")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Panel {
                    background: SurfaceRole::Raised
                    border_color: BorderRole::Default
                    border_width: 1.0
                    corner_radius: 8.0
                    padding: 16.0
                    child_id: language_switcher_body_id
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("ShortcutSettings")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Panel {
                    background: SurfaceRole::Raised
                    border_color: BorderRole::Default
                    border_width: 1.0
                    corner_radius: 8.0
                    padding: 16.0
                    child_id: shortcut_body_id
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("PrivacySettings")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Panel {
                    background: SurfaceRole::Raised
                    border_color: BorderRole::Default
                    border_width: 1.0
                    corner_radius: 8.0
                    padding: 16.0
                    child_id: privacy_body_id
                }
            }
        }
    )
}
