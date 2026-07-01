// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Over-constraint handling showcase — graceful shrink, layout priority,
//! height-for-width, and the inspector's overflow overlay.
//!
//! Run with: `cargo run -p over-constraint` (use a **debug** build so the
//! inspector and its overflow hazard stripes are available).
//!
//! Resize the window narrower to watch each row react:
//!
//! 1. **Toolbar** — a `Toolbar` of action commands. Buttons are rigid (a
//!    truncated action reads poorly), so when the bar is too narrow the excess
//!    actions collapse into a trailing `⌄` overflow menu (the desktop
//!    convention), reappearing as it widens.
//! 2. **Priority** — a `Shrinkable` label paired with a rigid icon button.
//!    The label gives up all its space first; the icon never shrinks
//!    ("compress this before that").
//! 3. **Height-for-width** — a `Shrinkable` wrapping a wrapping `TextWidget`.
//!    As it narrows, the paragraph re-wraps and grows taller, and the row
//!    grows with it (correct height propagated up the tree).
//! 4. **Residual overflow** — a row of rigid (non-shrinkable) blocks wider
//!    than the window. Nothing can shrink, so it overflows — press **F12**
//!    and you'll see yellow/black hazard stripes on the overflowing edge.

use bastyde::prelude::*;
use bastyde::widgets::{
    Card, Center, ComboBox, FixedSize, HStack, IconButton, IconWidget, Padding, RectWidget,
    Shrinkable, TextWidget, Toolbar, ToolbarAction, ToolbarItem, VStack,
};

/// A rigid square block standing in for a fixed-size action control (icon
/// button) — it never shrinks, so it shows off layout priority.
fn rigid_block(glyph: &'static str, size: f32) -> impl Widget + 'static {
    Card::new().content(
        FixedSize::new()
            .bind_width(size)
            .bind_height(size)
            .child(Center::new().child(TextWidget::new(lit!(glyph)).no_shrink())),
    )
}

fn section(title: &str, body: impl Widget + 'static) -> impl Widget + 'static {
    Card::new().content(
        VStack::new()
            .spacing(8.0)
            .child(TextWidget::new(lit!(title)).style(TextStyleRole::BodyBold))
            .child(body),
    )
}

/// 1. A `Toolbar` (rigid buttons): excess *actions* collapse into a trailing
///    `⌄` overflow menu — the desktop convention — instead of truncating, and
///    collapsible *widgets* show themselves in that menu. Tab enters the bar
///    once; Left/Right arrows rove between the controls (ARIA toolbar pattern).
fn toolbar_row() -> impl Widget + 'static {
    let view_mode = Signal::new(Some("List".to_string()));
    let menu_mode = view_mode.clone();
    Toolbar::new()
        // Collapsible ComboBox — when it overflows, the SAME control (bound to
        // the same signal) is embedded *live* in the chevron menu via
        // `overflow_widget`, so it stays fully usable while collapsed.
        .item(
            ToolbarItem::custom(ComboBox::new(["List", "Grid", "Columns"], view_mode))
                .overflow_widget(move || {
                    Box::new(ComboBox::new(
                        ["List", "Grid", "Columns"],
                        menu_mode.clone(),
                    ))
                }),
        )
        // Collapsible IconButton — its overflow row reuses the same icon as the
        // menu item's leading glyph (NSToolbar `menuFormRepresentation`).
        .item(
            ToolbarItem::custom(
                IconButton::new(IconWidget::checkmark(16.0)).tooltip(lit!("Confirm")),
            )
            .overflow_as(
                ToolbarAction::new(lit!("Confirm"), || IconWidget::checkmark(16.0))
                    .on_activate(|_| {}),
            ),
        )
        .item(ToolbarItem::separator())
        // Collapsible actions — overflow into the chevron menu.
        .action(
            ToolbarAction::new(lit!("New Document"), || IconWidget::checkmark(16.0))
                .on_activate(|_| {}),
        )
        .action(
            ToolbarAction::new(lit!("Open Recent Project…"), || {
                IconWidget::chevron_down(16.0)
            })
            .on_activate(|_| {}),
        )
        .action(
            ToolbarAction::new(lit!("Save Document As…"), || IconWidget::chevron_up(16.0))
                .on_activate(|_| {}),
        )
        .action(
            ToolbarAction::new(lit!("Export to PDF"), || IconWidget::chevron_right(16.0))
                .on_activate(|_| {}),
        )
        .action(
            ToolbarAction::new(lit!("Print Preview"), || IconWidget::chevron_left(16.0))
                .on_activate(|_| {}),
        )
}

/// 2. Priority: the label absorbs the deficit down to a 60px floor; the icon
///    button is rigid and keeps its size.
fn priority_row() -> impl Widget + 'static {
    HStack::new()
        .spacing(8.0)
        .child(
            Shrinkable::new().min_width(60.0).child(
                TextWidget::new(lit!(
                    "This descriptive label yields space before the action button does"
                ))
                .single_line(),
            ),
        )
        .child(rigid_block("★", 40.0))
}

/// 3. Height-for-width: a wrapping paragraph that re-wraps (and grows taller)
///    as the Shrinkable narrows it.
fn height_for_width_row() -> impl Widget + 'static {
    HStack::new()
        .spacing(8.0)
        .child(
            Shrinkable::new().min_width(80.0).child(TextWidget::new(lit!(
                "When this column is squeezed it re-wraps onto more lines and the row grows taller — height-for-width, propagated correctly up the layout tree."
            ))),
        )
        .child(rigid_block("✎", 32.0))
}

/// 4. Rigid blocks wider than the window: nothing can shrink, so the row
///    overflows. F12 → the inspector paints hazard stripes on the overhang.
fn residual_overflow_row() -> impl Widget + 'static {
    let block = |label: &'static str| {
        Card::new().content(
            FixedSize::new()
                .bind_width(220.0)
                .bind_height(44.0)
                .child(Center::new().child(TextWidget::new(lit!(label)).no_shrink())),
        )
    };
    HStack::new()
        .spacing(8.0)
        .child(block("rigid 220"))
        .child(block("rigid 220"))
        .child(block("rigid 220"))
        .child(RectWidget::new())
}

fn main() {
    BastydeAppBuilder::new()
        .install_automation_bridge_in_debug()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Over-constraint handling")
                .size(900, 640)
                .root(|tree, _state| {
                    tree.add(
                        Padding::uniform(16.0).child(
                            VStack::new()
                                .spacing(14.0)
                                .child(TextWidget::new(lit!(
                                    "Resize the window narrower. Press F12 for the overflow overlay."
                                )))
                                .child(section("1 · Toolbar overflows actions into a ⌄ menu", toolbar_row()))
                                .child(section("2 · Layout priority (label before icon)", priority_row()))
                                .child(section("3 · Height-for-width (re-wraps taller)", height_for_width_row()))
                                .child(section("4 · Residual overflow (hazard stripes under F12)", residual_overflow_row())),
                        ),
                    )
                }),
        )
        .run();
}
