// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Accessibility helpers for [`DockingLayout`](super::DockingLayout): the
//! localized landmark names for each side region and rail.

use teksilo_i18n::{LocalizedString, lit};

use super::geometry::DockSide;

/// The `Role::Complementary` landmark name for a side's content region.
pub(crate) fn side_label(side: DockSide) -> LocalizedString {
    match side {
        DockSide::Leading => lit!("Leading panel"),
        DockSide::Trailing => lit!("Trailing panel"),
        DockSide::Top => lit!("Top panel"),
        DockSide::Bottom => lit!("Bottom panel"),
    }
}

/// The `Role::TabList` name for a side's activity rail.
pub(crate) fn rail_label(side: DockSide) -> LocalizedString {
    match side {
        DockSide::Leading => lit!("Leading activity bar"),
        DockSide::Trailing => lit!("Trailing activity bar"),
        DockSide::Top => lit!("Top activity bar"),
        DockSide::Bottom => lit!("Bottom activity bar"),
    }
}

/// The `Role::Toolbar` name for a rail's dockless-action cluster. Named per
/// placement as well as per side: a rail may show more than one group, and two
/// identically-named toolbars in the same region are indistinguishable to a
/// screen reader's landmark list.
pub(crate) fn rail_actions_label(
    side: DockSide,
    placement: super::activity_bar::DockActionPlacement,
) -> LocalizedString {
    use super::activity_bar::DockActionPlacement as P;
    match (side, placement) {
        (DockSide::Leading, P::Start) => lit!("Leading activity bar actions (top)"),
        (DockSide::Leading, P::End) => lit!("Leading activity bar actions"),
        (DockSide::Leading, P::Pinned) => lit!("Leading activity bar actions (bottom)"),
        (DockSide::Trailing, P::Start) => lit!("Trailing activity bar actions (top)"),
        (DockSide::Trailing, P::End) => lit!("Trailing activity bar actions"),
        (DockSide::Trailing, P::Pinned) => lit!("Trailing activity bar actions (bottom)"),
        (DockSide::Top, P::Start) => lit!("Top activity bar actions (top)"),
        (DockSide::Top, P::End) => lit!("Top activity bar actions"),
        (DockSide::Top, P::Pinned) => lit!("Top activity bar actions (bottom)"),
        (DockSide::Bottom, P::Start) => lit!("Bottom activity bar actions (top)"),
        (DockSide::Bottom, P::End) => lit!("Bottom activity bar actions"),
        (DockSide::Bottom, P::Pinned) => lit!("Bottom activity bar actions (bottom)"),
    }
}
