// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Accessibility names for [`DockingLayout`](super::DockingLayout): the side
//! regions' landmarks, their activity bars and action groups, and the sides
//! as the "Move to" menus offer them.
//!
//! A side is named for where it is on screen, not for its logical place: the
//! leading side is "Left panel" left to right and "Right panel" right to left,
//! read from the direction the layout last placed it in. A side the
//! application named with `DockingLayout::side_label` takes that name, and
//! its activity bar and actions are named after it. Every name is resolved
//! when asked for, so it follows a locale switch.

use teksilo_i18n::tr_widget;

use super::activity_bar::DockActionPlacement;
use super::geometry::DockSide;
use super::model::DockingModel;

/// Where a side is on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScreenSide {
    Left,
    Right,
    Top,
    Bottom,
}

/// The screen side of a logical side: leading is left, trailing right, both
/// mirrored in right-to-left.
pub(crate) fn screen_side(side: DockSide, rtl: bool) -> ScreenSide {
    match (side, rtl) {
        (DockSide::Leading, false) | (DockSide::Trailing, true) => ScreenSide::Left,
        (DockSide::Trailing, false) | (DockSide::Leading, true) => ScreenSide::Right,
        (DockSide::Top, _) => ScreenSide::Top,
        (DockSide::Bottom, _) => ScreenSide::Bottom,
    }
}

/// The application's name for `side`, resolved now, if it gave one.
fn app_label(model: &DockingModel, side: DockSide) -> Option<String> {
    // Cloned out of the model before resolving, so a label that reads the
    // model meets no held borrow.
    let label = model.app_side_label(side)?;
    let text = label.resolve_now();
    (!text.is_empty()).then_some(text)
}

/// The `Role::Complementary` landmark name for a side's content region.
pub(crate) fn side_label(model: &DockingModel, side: DockSide) -> String {
    if let Some(label) = app_label(model, side) {
        return label;
    }
    match screen_side(side, model.layout_rtl()) {
        ScreenSide::Left => tr_widget!(dock_side_left()),
        ScreenSide::Right => tr_widget!(dock_side_right()),
        ScreenSide::Top => tr_widget!(dock_side_top()),
        ScreenSide::Bottom => tr_widget!(dock_side_bottom()),
    }
    .resolve_now()
}

/// The `Role::TabList` name for a side's activity rail.
pub(crate) fn rail_label(model: &DockingModel, side: DockSide) -> String {
    if let Some(label) = app_label(model, side) {
        return tr_widget!(dock_rail_named(label = label)).resolve_now();
    }
    match screen_side(side, model.layout_rtl()) {
        ScreenSide::Left => tr_widget!(dock_rail_left()),
        ScreenSide::Right => tr_widget!(dock_rail_right()),
        ScreenSide::Top => tr_widget!(dock_rail_top()),
        ScreenSide::Bottom => tr_widget!(dock_rail_bottom()),
    }
    .resolve_now()
}

/// The `Role::Toolbar` name for a rail's dockless-action cluster. Named per
/// placement as well as per side: a rail may show more than one group, and two
/// identically-named toolbars in the same region are indistinguishable to a
/// screen reader's landmark list.
pub(crate) fn rail_actions_label(
    model: &DockingModel,
    side: DockSide,
    placement: DockActionPlacement,
) -> String {
    let placement = match placement {
        DockActionPlacement::Start => "start",
        DockActionPlacement::End => "end",
        DockActionPlacement::Pinned => "pinned",
    }
    .to_string();
    if let Some(label) = app_label(model, side) {
        return tr_widget!(dock_rail_actions_named(
            label = label,
            placement = placement
        ))
        .resolve_now();
    }
    match screen_side(side, model.layout_rtl()) {
        ScreenSide::Left => tr_widget!(dock_rail_actions_left(placement = placement)),
        ScreenSide::Right => tr_widget!(dock_rail_actions_right(placement = placement)),
        ScreenSide::Top => tr_widget!(dock_rail_actions_top(placement = placement)),
        ScreenSide::Bottom => tr_widget!(dock_rail_actions_bottom(placement = placement)),
    }
    .resolve_now()
}

/// A side as the "Move to" menus offer it: the application's name for it,
/// else where it is on screen ("Left", "Bottom").
pub(crate) fn move_target_label(model: &DockingModel, side: DockSide) -> String {
    if let Some(label) = app_label(model, side) {
        return label;
    }
    match screen_side(side, model.layout_rtl()) {
        ScreenSide::Left => tr_widget!(dock_move_left()),
        ScreenSide::Right => tr_widget!(dock_move_right()),
        ScreenSide::Top => tr_widget!(dock_move_top()),
        ScreenSide::Bottom => tr_widget!(dock_move_bottom()),
    }
    .resolve_now()
}
