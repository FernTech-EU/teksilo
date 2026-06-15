// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tab registry — defines the canonical list of tabs and exposes a
//! pair of `(classic, bati)` builder functions for each.
//!
//! Each tab module exports:
//!
//! ```ignore
//! pub fn classic(ctx: &mut BuildContext, sigs: &Signals) -> WidgetId;
//! pub fn bati(ctx: &mut BuildContext, sigs: &Signals) -> WidgetId;
//! ```
//!
//! `main.rs` walks `TABS` to construct the `TabWidget`, calling both
//! functions per tab and wrapping them in a `Switcher` driven by the
//! shared view-mode signal.

use bastyde::prelude::*;

use crate::shared::Signals;

pub mod animations;
pub mod buttons;
pub mod charts;
pub mod chrome;
pub mod color;
pub mod containers;
pub mod data;
pub mod datetime;
pub mod dragdrop;
pub mod indicators;
pub mod inputs;
pub mod layout;
pub mod menus;
pub mod overlays;
pub mod palette;
pub mod richtext;
pub mod scene;
pub mod settings;
pub mod styling;
pub mod text;
pub mod visuals;

/// One tab entry. `title_fn` and `refs_fn` return reactive
/// `LocalizedString`s built via `tr!` — they re-resolve every
/// build, so locale switches retitle the tabs live. `classic` / `bati`
/// build the tab body for the two view modes.
#[derive(Debug)]
pub struct TabEntry {
    pub lowercase_name: &'static str,
    pub title_fn: fn() -> LocalizedString,
    pub refs_fn: fn() -> LocalizedString,
    pub classic: fn(&mut BuildContext, &Signals) -> WidgetId,
    pub bati: fn(&mut BuildContext, &Signals) -> WidgetId,
}

/// Canonical tab list. Order is stable — `--tab N` indexes into this.
pub static TABS: &[TabEntry] = &[
    TabEntry {
        lowercase_name: "palette",
        title_fn: palette::title,
        refs_fn: palette::refs,
        classic: palette::classic,
        bati: palette::bati,
    },
    TabEntry {
        lowercase_name: "layout",
        title_fn: layout::title,
        refs_fn: layout::refs,
        classic: layout::classic,
        bati: layout::bati,
    },
    TabEntry {
        lowercase_name: "visuals",
        title_fn: visuals::title,
        refs_fn: visuals::refs,
        classic: visuals::classic,
        bati: visuals::bati,
    },
    TabEntry {
        lowercase_name: "containers",
        title_fn: containers::title,
        refs_fn: containers::refs,
        classic: containers::classic,
        bati: containers::bati,
    },
    TabEntry {
        lowercase_name: "chrome",
        title_fn: chrome::title,
        refs_fn: chrome::refs,
        classic: chrome::classic,
        bati: chrome::bati,
    },
    TabEntry {
        lowercase_name: "buttons",
        title_fn: buttons::title,
        refs_fn: buttons::refs,
        classic: buttons::classic,
        bati: buttons::bati,
    },
    TabEntry {
        lowercase_name: "styling",
        title_fn: styling::title,
        refs_fn: styling::refs,
        classic: styling::classic,
        bati: styling::bati,
    },
    TabEntry {
        lowercase_name: "inputs",
        title_fn: inputs::title,
        refs_fn: inputs::refs,
        classic: inputs::classic,
        bati: inputs::bati,
    },
    TabEntry {
        lowercase_name: "indicators",
        title_fn: indicators::title,
        refs_fn: indicators::refs,
        classic: indicators::classic,
        bati: indicators::bati,
    },
    TabEntry {
        lowercase_name: "charts",
        title_fn: charts::title,
        refs_fn: charts::refs,
        classic: charts::classic,
        bati: charts::bati,
    },
    TabEntry {
        lowercase_name: "scene",
        title_fn: scene::title,
        refs_fn: scene::refs,
        classic: scene::classic,
        bati: scene::bati,
    },
    TabEntry {
        lowercase_name: "text",
        title_fn: text::title,
        refs_fn: text::refs,
        classic: text::classic,
        bati: text::bati,
    },
    TabEntry {
        lowercase_name: "richtext",
        title_fn: richtext::title,
        refs_fn: richtext::refs,
        classic: richtext::classic,
        bati: richtext::bati,
    },
    TabEntry {
        lowercase_name: "datetime",
        title_fn: datetime::title,
        refs_fn: datetime::refs,
        classic: datetime::classic,
        bati: datetime::bati,
    },
    TabEntry {
        lowercase_name: "color",
        title_fn: color::title,
        refs_fn: color::refs,
        classic: color::classic,
        bati: color::bati,
    },
    TabEntry {
        lowercase_name: "menus",
        title_fn: menus::title,
        refs_fn: menus::refs,
        classic: menus::classic,
        bati: menus::bati,
    },
    TabEntry {
        lowercase_name: "overlays",
        title_fn: overlays::title,
        refs_fn: overlays::refs,
        classic: overlays::classic,
        bati: overlays::bati,
    },
    TabEntry {
        lowercase_name: "data",
        title_fn: data::title,
        refs_fn: data::refs,
        classic: data::classic,
        bati: data::bati,
    },
    TabEntry {
        lowercase_name: "dragdrop",
        title_fn: dragdrop::title,
        refs_fn: dragdrop::refs,
        classic: dragdrop::classic,
        bati: dragdrop::bati,
    },
    TabEntry {
        lowercase_name: "animations",
        title_fn: animations::title,
        refs_fn: animations::refs,
        classic: animations::classic,
        bati: animations::bati,
    },
    TabEntry {
        lowercase_name: "settings",
        title_fn: settings::title,
        refs_fn: settings::refs,
        classic: settings::classic,
        bati: settings::bati,
    },
];

/// Lowercase tab names — used by `cli::parse` to resolve `--tab NAME`.
pub fn tab_names() -> Vec<&'static str> {
    TABS.iter().map(|t| t.lowercase_name).collect()
}
