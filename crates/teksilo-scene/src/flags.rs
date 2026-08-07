// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Per-item behavior flags.
//!
//! [`ItemFlags`] is a bitset packed into a `u32`. Each flag opts an
//! item into a behavior — drag-to-move participation, hit-test
//! response, rendering visibility, transform inheritance — that
//! the Scene and SceneView consult at the relevant pipeline stage.
//!
//! Defaults: `IS_VISIBLE | IS_ENABLED | IS_SELECTABLE`. An item
//! constructed via the standard built-in builders gets these
//! defaults; setters layer additional flags on top.

/// A bitset of per-item behavior flags.
///
/// Use [`ItemFlags::default`] for the standard "interactive,
/// visible, selectable" baseline. Compose flags with `|` and toggle
/// them with [`ItemFlags::set`] / [`ItemFlags::contains`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ItemFlags(u32);

impl ItemFlags {
    /// Empty bitset — no flags set.
    pub const NONE: Self = Self(0);

    /// Item paints and is hit-tested. Default on. Clearing this is
    /// the equivalent of Qt's `setVisible(false)` — the item is
    /// neither painted nor hit-tested. Children of an invisible
    /// item are also effectively invisible.
    pub const IS_VISIBLE: Self = Self(1 << 0);

    /// Item dispatches pointer events. Default on. Disabled items
    /// are still painted but pass clicks through to items beneath.
    pub const IS_ENABLED: Self = Self(1 << 1);

    /// Item participates in drag-to-move. Default off.
    pub const IS_DRAGGABLE: Self = Self(1 << 2);

    /// Item is included in marquee box-select results. Default on.
    pub const IS_SELECTABLE: Self = Self(1 << 3);

    /// Item can take keyboard focus. Default off; the focus_order
    /// callback considers only items with this flag set.
    pub const IS_FOCUSABLE: Self = Self(1 << 4);

    /// Item dispatches hover events (Qt `setAcceptHoverEvents`).
    /// Default off; hover handlers wired via `ItemBuilder::on_hover`
    /// flip this on automatically.
    pub const ACCEPTS_HOVER: Self = Self(1 << 5);

    /// Item's paint output is clipped to its `local_bounds`.
    /// Default off.
    pub const CLIPS_TO_SHAPE: Self = Self(1 << 6);

    /// Children are clipped to this item's `local_bounds`. Default
    /// off; mirrors Qt's `ItemClipsChildrenToShape`.
    pub const CLIPS_CHILDREN_TO_SHAPE: Self = Self(1 << 7);

    /// Item paints and hit-tests at a fixed pixel size, independent
    /// of the view's zoom and rotation. Its anchor (the item's
    /// parent-relative scene point) is projected through the view
    /// transform like any other point, so the visible position
    /// follows pan/zoom and tracks the underlying scene data —
    /// but the item itself does not grow with zoom or rotate with
    /// the view. Mirrors Qt's `ItemIgnoresTransformations`.
    /// Annotation pins for graph editors, fixed-pixel-size badges
    /// over moving content, chart axis labels. Default off.
    pub const IGNORES_TRANSFORMATIONS: Self = Self(1 << 8);

    /// Item has nothing to paint — the paint walk skips it
    /// entirely. Pure logical-only containers (used for AT
    /// grouping or hit-test routing) set this. Default off.
    pub const HAS_NO_CONTENTS: Self = Self(1 << 9);

    /// Children with `z < 0` paint **behind** this item rather
    /// than in front. Mirrors Qt's `ItemNegativeZStacksBehindParent`.
    /// Default off.
    pub const NEGATIVE_Z_BEHIND_PARENT: Self = Self(1 << 10);

    /// Whether the bitset contains every flag in `other`.
    pub const fn contains(&self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Whether the bitset shares any flags with `other`.
    pub const fn intersects(&self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }

    /// Set (when `on`) or clear (when `!on`) the bits in `flag`.
    pub fn set(&mut self, flag: Self, on: bool) {
        if on {
            self.0 |= flag.0;
        } else {
            self.0 &= !flag.0;
        }
    }

    /// Set the bits in `flag`, returning the new bitset.
    pub const fn with(self, flag: Self) -> Self {
        Self(self.0 | flag.0)
    }

    /// Clear the bits in `flag`, returning the new bitset.
    pub const fn without(self, flag: Self) -> Self {
        Self(self.0 & !flag.0)
    }

    /// Raw `u32` bits (debug / serialization).
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Construct from raw bits.
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }
}

impl Default for ItemFlags {
    /// `IS_VISIBLE | IS_ENABLED | IS_SELECTABLE`.
    fn default() -> Self {
        Self::IS_VISIBLE
            .with(Self::IS_ENABLED)
            .with(Self::IS_SELECTABLE)
    }
}

impl std::ops::BitOr for ItemFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for ItemFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl std::ops::BitAnd for ItemFlags {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_carries_visible_enabled_selectable() {
        let f = ItemFlags::default();
        assert!(f.contains(ItemFlags::IS_VISIBLE));
        assert!(f.contains(ItemFlags::IS_ENABLED));
        assert!(f.contains(ItemFlags::IS_SELECTABLE));
        assert!(!f.contains(ItemFlags::IS_DRAGGABLE));
        assert!(!f.contains(ItemFlags::IS_FOCUSABLE));
    }

    #[test]
    fn set_toggles_individual_bits() {
        let mut f = ItemFlags::default();
        f.set(ItemFlags::IS_DRAGGABLE, true);
        assert!(f.contains(ItemFlags::IS_DRAGGABLE));
        f.set(ItemFlags::IS_VISIBLE, false);
        assert!(!f.contains(ItemFlags::IS_VISIBLE));
        assert!(f.contains(ItemFlags::IS_ENABLED));
    }

    #[test]
    fn with_without_round_trip() {
        let f = ItemFlags::default()
            .with(ItemFlags::IS_DRAGGABLE)
            .with(ItemFlags::IGNORES_TRANSFORMATIONS);
        assert!(f.contains(ItemFlags::IS_DRAGGABLE));
        assert!(f.contains(ItemFlags::IGNORES_TRANSFORMATIONS));
        let f = f.without(ItemFlags::IS_DRAGGABLE);
        assert!(!f.contains(ItemFlags::IS_DRAGGABLE));
        assert!(f.contains(ItemFlags::IGNORES_TRANSFORMATIONS));
    }

    #[test]
    fn intersects_detects_any_overlap() {
        let f = ItemFlags::IS_VISIBLE | ItemFlags::IS_DRAGGABLE;
        assert!(f.intersects(ItemFlags::IS_DRAGGABLE));
        assert!(f.intersects(ItemFlags::IS_VISIBLE | ItemFlags::IS_ENABLED));
        assert!(!f.intersects(ItemFlags::IS_FOCUSABLE));
    }
}
