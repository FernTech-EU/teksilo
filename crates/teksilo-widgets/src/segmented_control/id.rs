// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Stable per-segment identifiers.
//!
//! [`SegmentId`] is the identity of a segment as segments are added,
//! removed, reordered, or *contributed* by another crate. Selection, the
//! `on_change` callback, and the overflow menu are all keyed by
//! `SegmentId` rather than by position — so inserting a segment never
//! silently re-points the selection at a different one.
//!
//! Mirrors [`TabId`](crate::tab_widget::TabId): apps either let the
//! framework allocate fresh ids ([`SegmentId::fresh`]) or wrap their own
//! external keys via [`SegmentId::from_raw`] / [`SegmentId::from_u64`].

use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};

/// Stable identity of a segment. Cheap to copy; survives rebuilds,
/// locale changes, and segments being inserted around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SegmentId(NonZeroU64);

/// Framework-allocated ids start here, leaving everything below it to
/// apps. Without the split, `SegmentId::from_u64(1)` — the obvious first
/// constant anyone writes — would collide with the first
/// [`SegmentId::fresh`] of the process.
const FRESH_BASE: u64 = 1 << 48;

impl SegmentId {
    /// Allocate a new, never-before-seen id. Backed by a monotonic
    /// global counter — overflow is theoretically possible after 2^64
    /// calls, at which point the universe has had bigger problems.
    ///
    /// [`Segment::new`](super::Segment::new) calls this for you, so a
    /// control that never persists its selection needs no explicit ids.
    ///
    /// Allocations start at 2^48, so they can never collide with a small
    /// constant an app declared through [`from_u64`](Self::from_u64).
    pub fn fresh() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(FRESH_BASE);
        let raw = COUNTER.fetch_add(1, Ordering::Relaxed);
        // The counter starts well above zero and only ever increments, so
        // the value is non-zero in any practical run.
        Self(NonZeroU64::new(raw).expect("SegmentId counter wrapped to zero"))
    }

    /// Wrap an externally-allocated key. Use this when the segment's
    /// identity comes from an app-side store (a view-mode enum
    /// discriminant, a plugin key hash, …) — calling [`SegmentId::fresh`]
    /// would allocate a *new* id every restart, breaking a persisted
    /// selection.
    pub const fn from_raw(value: NonZeroU64) -> Self {
        Self(value)
    }

    /// `const` convenience over [`from_raw`](Self::from_raw), so an app
    /// can declare its segments as constants:
    ///
    /// ```
    /// # use teksilo_widgets::SegmentId;
    /// const SYNOPSIS: SegmentId = SegmentId::from_u64(1);
    /// const CHAPTER: SegmentId = SegmentId::from_u64(2);
    /// ```
    ///
    /// # Panics
    ///
    /// If `value` is zero. Because this is a `const fn`, a literal zero
    /// is caught at compile time rather than at run time.
    pub const fn from_u64(value: u64) -> Self {
        match NonZeroU64::new(value) {
            Some(v) => Self(v),
            None => panic!("SegmentId::from_u64 requires a non-zero value"),
        }
    }

    /// The underlying non-zero `u64`. Serialize this to persist a
    /// selection across sessions; restore via [`from_raw`](Self::from_raw)
    /// or [`from_u64`](Self::from_u64).
    pub const fn raw(self) -> NonZeroU64 {
        self.0
    }

    /// The underlying value as a plain `u64`.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl std::fmt::Display for SegmentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SegmentId({})", self.0.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_ids_are_unique_and_non_zero() {
        let a = SegmentId::fresh();
        let b = SegmentId::fresh();
        assert_ne!(a, b);
        assert!(a.get() > 0);
        assert!(b.get() > 0);
    }

    #[test]
    fn raw_round_trips() {
        let id = SegmentId::from_u64(42);
        assert_eq!(id.get(), 42);
        assert_eq!(SegmentId::from_raw(id.raw()), id);
    }

    #[test]
    fn const_construction_is_usable_in_a_const_item() {
        const A: SegmentId = SegmentId::from_u64(7);
        assert_eq!(A.get(), 7);
    }

    #[test]
    fn fresh_ids_never_collide_with_small_app_constants() {
        // `from_u64(1)` is the first constant anyone writes; a counter
        // starting at 1 would hand out the same id to an unrelated
        // segment and silently merge two selections.
        const APP: SegmentId = SegmentId::from_u64(1);
        for _ in 0..64 {
            assert_ne!(SegmentId::fresh(), APP);
        }
        assert!(SegmentId::fresh().get() >= FRESH_BASE);
    }
}
