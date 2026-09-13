// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The text-affordance overlay band: where selection handles, the magnifier
//! and the selection toolbar live.
//!
//! Touch text editing needs three pieces of chrome that a text widget cannot
//! paint itself. Each of them fails in the same two ways when painted by the
//! host:
//!
//! * **They are clipped.** Every real editor sits inside something with
//!   `clips_children` — a `ScrollArea`, a `MaxSize`, a docked pane. A handle
//!   hangs *below* the last line and a magnifier hangs *above* the caret, so
//!   both are exactly the geometry the clip removes.
//! * **They are unreachable.** A handle is a 24 dp circle with a 44 dp hit
//!   rect. Inside the editor that rect is competing with the editor's own
//!   caret placement; outside it, in an overlay, it is a target in its own
//!   right.
//!
//! So they are overlays. But they must not behave like menus: an overlay is
//! normally torn down by a press outside it, and *every* press that moves the
//! caret is outside a selection handle. Hence a band rather than a plain
//! overlay — [`OverlayBand::TextAffordance`] sorts below every menu, dialog
//! and tooltip, and is exempt from outside-press dismissal. Its lifetime
//! belongs to the controller that raised it (the `TouchSelection` contract),
//! which dismisses it explicitly.
//!
//! This module owns the band and the handle vocabulary. The chrome that fills
//! it is built on top.

use crate::environment::LayoutDirection;

use super::direction::{HorizontalSide, InlineDirection};

/// Which z-band an overlay sits in.
///
/// Bands sort before show order: an overlay is inserted after every overlay in
/// a lower band and before every overlay in a higher one, so a selection handle
/// raised while a menu is open still goes *under* the menu. Within a band the
/// stack keeps its historical push order, which is why a tree that never raises
/// a text affordance behaves exactly as it did before bands existed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum OverlayBand {
    /// Selection handles, the magnifier and the selection toolbar. Above the
    /// content but below every menu, and never dismissed by a press outside
    /// itself — see the [module docs](self).
    TextAffordance,
    /// Menus, dialogs, popovers, tooltips, toasts, drag previews — everything
    /// that existed before the band split, and the default.
    #[default]
    Standard,
}

impl OverlayBand {
    /// Whether an outside press may close overlays in this band.
    ///
    /// False for [`TextAffordance`](Self::TextAffordance): every caret-moving
    /// tap is "outside" a selection handle, so outside-press dismissal would
    /// retire the handles on the first tap that used them.
    pub const fn dismissed_by_outside_press(self) -> bool {
        matches!(self, OverlayBand::Standard)
    }
}

/// Which of a selection's affordances a handle is.
///
/// `Start` and `End` are stated in *logical* order — the start of the selection
/// is the offset the user anchored at, whatever side of the screen that ends up
/// on. [`side`](Self::side) is the only place that becomes a physical side, so
/// a mirror can never be applied twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SelectionHandleKind {
    /// The single handle under a collapsed caret. Has no side.
    Caret,
    /// The handle at the lower text offset of the selection.
    Start,
    /// The handle at the higher text offset of the selection.
    End,
}

impl SelectionHandleKind {
    /// Which end of the reading axis this handle sits at, or `None` for the
    /// caret handle, which sits under the caret and has no side.
    pub const fn inline(self) -> Option<InlineDirection> {
        match self {
            SelectionHandleKind::Caret => None,
            SelectionHandleKind::Start => Some(InlineDirection::Backward),
            SelectionHandleKind::End => Some(InlineDirection::Forward),
        }
    }

    /// Which physical side of the selection this handle is painted on.
    ///
    /// `Start` is on the left in English and on the **right** in Arabic: the
    /// selection's first character is where the line begins, and that is the
    /// right-hand edge under RTL. Anything that draws a handle asks this rather
    /// than assuming; anything that hit-tests one asks it too, so the two
    /// cannot disagree.
    pub const fn side(self, direction: LayoutDirection) -> Option<HorizontalSide> {
        match self.inline() {
            Some(inline) => Some(inline.side(direction)),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LTR: LayoutDirection = LayoutDirection::LeftToRight;
    const RTL: LayoutDirection = LayoutDirection::RightToLeft;

    #[test]
    fn selection_handles_map_to_physical_sides_by_direction() {
        assert_eq!(
            SelectionHandleKind::Start.side(LTR),
            Some(HorizontalSide::Left)
        );
        assert_eq!(
            SelectionHandleKind::Start.side(RTL),
            Some(HorizontalSide::Right)
        );
        assert_eq!(
            SelectionHandleKind::End.side(LTR),
            Some(HorizontalSide::Right)
        );
        assert_eq!(
            SelectionHandleKind::End.side(RTL),
            Some(HorizontalSide::Left)
        );
        assert_eq!(SelectionHandleKind::Caret.side(LTR), None);
        assert_eq!(SelectionHandleKind::Caret.side(RTL), None);
    }

    /// The two handles are never on the same side, in either script — the
    /// invariant a hit-test that picks "the nearer handle" depends on.
    #[test]
    fn the_two_handles_are_always_on_opposite_sides() {
        for direction in [LTR, RTL] {
            let start = SelectionHandleKind::Start.side(direction).unwrap();
            let end = SelectionHandleKind::End.side(direction).unwrap();
            assert_eq!(start, end.opposite(), "under {direction:?}");
        }
    }

    #[test]
    fn only_the_standard_band_answers_to_an_outside_press() {
        assert!(OverlayBand::Standard.dismissed_by_outside_press());
        assert!(!OverlayBand::TextAffordance.dismissed_by_outside_press());
        assert_eq!(OverlayBand::default(), OverlayBand::Standard);
        assert!(OverlayBand::TextAffordance < OverlayBand::Standard);
    }
}
