//! Accessibility helpers for [`DockingLayout`](super::DockingLayout): the
//! localized landmark names for each side region and rail.

use bastyde_i18n::{LocalizedString, lit};

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
