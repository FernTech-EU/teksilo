// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`PalmWatch`] — the conservative fallback for a backend that cannot tell a
//! palm from a finger.
//!
//! # Where palms are normally rejected
//!
//! At the translator, by the digitiser. Windows asks for `TWF_WANTPALM`,
//! libinput classifies palms itself, and both set
//! [`PointerInfo::palm`](crate::pointer::PointerInfo::palm), which
//! [`PointerTable::would_admit`](crate::pointer::table::PointerTable::would_admit)
//! refuses outright — the contact produces no event at all. That is one
//! decision in one place, and it is the right one whenever the hardware
//! answers.
//!
//! # When it does not answer
//!
//! Every [`BackendCaps`] row Teksilo ships today reports `reports_palm: false`:
//! winit 0.30 surfaces neither `TOUCH_FLAG_PALM` nor libinput's classification.
//! On those hosts a hand resting on a tablet is an ordinary contact, and left
//! alone it opens menus.
//!
//! So this watch, and note how little it claims. It rejects a contact only when
//! **both** of two things are true for its whole life:
//!
//! * the reported contact patch is larger than [`PALM_CONTACT_THRESHOLD`] on
//!   either axis — a heuristic, and only reachable at all on a backend that
//!   fills [`PointerAxes::contact`](crate::pointer::PointerAxes::contact);
//! * it never travelled past the profile's `tap_slop`.
//!
//! A palm that slides is not rejected, because a *deliberate* drag from a large
//! contact (a stylus barrel, a gloved finger, an accessibility switch) must
//! keep working; false-rejecting a real gesture is worse than passing a
//! stationary palm through. And the verdict is only read on **release**, so a
//! contact is never revoked while the user might still be doing something with
//! it.
//!
//! [`BackendCaps`]: https://docs.rs/teksilo-platform

use teksilo_canvas::Point;
use teksilo_tokens::GestureProfile;

use crate::pointer::PointerAxes;

/// A contact patch wider or taller than this many logical pixels is *large
/// enough to be a palm* for the fallback heuristic.
///
/// There is no upstream constant to reproduce: Android's palm rejection is
/// inside the digitiser HAL, libinput's thresholds are in millimetres against a
/// device-specific resolution, and Windows never exposes its own. 40 dp is the
/// smallest figure that is comfortably above a fingertip and comfortably below
/// a palm: Android's `ViewConfiguration` treats a **24 dp** target as
/// finger-sized, and Apple's HIG puts a fingertip at 44 pt, so a patch wider
/// than 40 dp on a display-density-independent axis is not a fingertip. It is
/// deliberately generous — the cost of setting it too low is refusing a real
/// touch, and the cost of setting it too high is only that a palm behaves as it
/// does today.
pub const PALM_CONTACT_THRESHOLD: f32 = 40.0;

/// Per-contact bookkeeping for the fallback: how big it ever got, and whether
/// it ever moved.
///
/// One of these per live direct pointer, owned by the tree. Cheap: two floats,
/// a point and a bool.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PalmWatch {
    /// Where the press landed, for the travel test.
    origin: Point,
    /// The largest contact-patch extent seen on this contact so far. A palm
    /// settling onto the glass grows over its first few samples, so the
    /// **maximum** is the honest reading — the first sample alone would miss
    /// it.
    largest: f32,
    /// Whether the contact has ever been further from `origin` than the
    /// profile's `tap_slop`. Latched: coming back does not un-move it.
    travelled: bool,
}

impl PalmWatch {
    /// Start watching a contact that pressed at `origin` with `axes`.
    pub fn press(origin: Point, axes: &PointerAxes) -> Self {
        Self {
            origin,
            largest: extent(axes),
            travelled: false,
        }
    }

    /// Fold in one sample.
    pub fn sample(&mut self, position: Point, axes: &PointerAxes, profile: &GestureProfile) {
        self.largest = self.largest.max(extent(axes));
        if !self.travelled {
            let dx = position.x - self.origin.x;
            let dy = position.y - self.origin.y;
            self.travelled = (dx * dx + dy * dy).sqrt() > profile.tap_slop;
        }
    }

    /// Whether this contact should be revoked as a palm rather than released.
    ///
    /// Read on the `Up`, never before.
    pub fn is_palm(&self) -> bool {
        self.largest > PALM_CONTACT_THRESHOLD && !self.travelled
    }

    /// Whether the contact has left the tap boundary. Exposed so a caller can
    /// see *why* a large contact was let through.
    pub fn travelled(&self) -> bool {
        self.travelled
    }

    /// The largest contact extent seen, in logical pixels. `0.0` on a backend
    /// that reports no contact size, which is what makes the whole heuristic a
    /// no-op there.
    pub fn largest_extent(&self) -> f32 {
        self.largest
    }
}

/// The larger of a contact ellipse's two axes, or `0.0` when the backend
/// reports no size at all.
///
/// Zero is the load-bearing default: a backend that reports nothing can never
/// exceed the threshold, so the fallback is inert rather than guessing.
fn extent(axes: &PointerAxes) -> f32 {
    axes.contact
        .map(|size| size.width.max(size.height))
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gesture::default_profile;
    use teksilo_canvas::Size;
    use teksilo_tokens::PointerKind;

    fn touch() -> GestureProfile {
        default_profile(PointerKind::Touch)
    }

    fn contact(extent: f32) -> PointerAxes {
        PointerAxes {
            pressure: None,
            tangential_pressure: None,
            tilt: None,
            twist: None,
            contact: Some(Size::new(extent, extent)),
        }
    }

    fn no_size() -> PointerAxes {
        PointerAxes {
            pressure: None,
            tangential_pressure: None,
            tilt: None,
            twist: None,
            contact: None,
        }
    }

    #[test]
    fn a_large_stationary_contact_is_a_palm() {
        let profile = touch();
        let mut watch = PalmWatch::press(Point::new(50.0, 50.0), &contact(60.0));
        // Jitter well inside `tap_slop` — a resting hand is never perfectly
        // still, and treating that as travel would defeat the whole watch.
        watch.sample(Point::new(51.0, 50.5), &contact(60.0), &profile);
        assert!(watch.is_palm());
    }

    #[test]
    fn a_large_contact_that_travels_is_left_alone() {
        let profile = touch();
        let mut watch = PalmWatch::press(Point::new(50.0, 50.0), &contact(60.0));
        watch.sample(
            Point::new(50.0, 50.0 + profile.tap_slop + 1.0),
            &contact(60.0),
            &profile,
        );
        assert!(watch.travelled());
        assert!(
            !watch.is_palm(),
            "a deliberate drag from a large contact must keep working"
        );
    }

    #[test]
    fn travel_is_latched_so_coming_back_does_not_undo_it() {
        let profile = touch();
        let mut watch = PalmWatch::press(Point::ZERO, &contact(60.0));
        watch.sample(
            Point::new(0.0, profile.tap_slop + 10.0),
            &contact(60.0),
            &profile,
        );
        watch.sample(Point::ZERO, &contact(60.0), &profile);
        assert!(watch.travelled());
        assert!(!watch.is_palm());
    }

    #[test]
    fn a_fingertip_is_never_a_palm() {
        let profile = touch();
        let mut watch = PalmWatch::press(Point::ZERO, &contact(12.0));
        watch.sample(Point::new(0.5, 0.5), &contact(14.0), &profile);
        assert!(!watch.is_palm());
    }

    #[test]
    fn a_contact_that_grows_into_a_palm_is_caught() {
        let profile = touch();
        // Settling onto the glass: the first sample is fingertip-sized.
        let mut watch = PalmWatch::press(Point::ZERO, &contact(10.0));
        watch.sample(Point::new(0.5, 0.0), &contact(35.0), &profile);
        watch.sample(Point::new(0.5, 0.5), &contact(70.0), &profile);
        assert_eq!(watch.largest_extent(), 70.0, "the maximum is what counts");
        assert!(watch.is_palm());
    }

    #[test]
    fn a_backend_that_reports_no_contact_size_makes_the_watch_inert() {
        let profile = touch();
        let mut watch = PalmWatch::press(Point::ZERO, &no_size());
        watch.sample(Point::new(0.2, 0.2), &no_size(), &profile);
        assert_eq!(watch.largest_extent(), 0.0);
        assert!(
            !watch.is_palm(),
            "with no size reported the heuristic must never fire"
        );
    }
}
