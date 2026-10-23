// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Which window the user was working with last.
//!
//! A window's `focused` flag cannot answer it. Every window is born with the
//! flag set, because winit does not send `Focused(true)` for a new window on
//! every platform, so two windows can claim focus at once; and the window map
//! is a `HashMap`, so picking "the focused one" out of it picked whichever
//! the hash order put first.
//!
//! The order kept here is total: each window gets a stamp when it opens and a
//! newer one whenever the OS reports it focused, and the newest stamp wins. A
//! window that closes leaves the order, so the one used before it takes over.

use std::collections::HashMap;

use crate::window_config::TeksiloWindowId;

/// The activation order of the open windows. See the module docs.
#[derive(Debug, Default)]
pub(crate) struct WindowRecency {
    /// The last stamp handed out. Stamps only grow, so two windows never
    /// share one.
    last: u64,
    /// Each open window's latest stamp.
    stamps: HashMap<TeksiloWindowId, u64>,
}

impl WindowRecency {
    /// `id` opened or took focus: it is now the most recent window.
    pub(crate) fn touch(&mut self, id: TeksiloWindowId) {
        self.last += 1;
        self.stamps.insert(id, self.last);
    }

    /// `id` closed. Forgetting an id never seen is a no-op.
    pub(crate) fn forget(&mut self, id: TeksiloWindowId) {
        self.stamps.remove(&id);
    }

    /// `id`'s place in the order, higher for a window used later; `None`
    /// for a window that is not open.
    pub(crate) fn stamp(&self, id: TeksiloWindowId) -> Option<u64> {
        self.stamps.get(&id).copied()
    }

    /// The window opened or focused last, `None` with no window open.
    pub(crate) fn most_recent(&self) -> Option<TeksiloWindowId> {
        self.stamps
            .iter()
            .max_by_key(|(_, stamp)| **stamp)
            .map(|(id, _)| *id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(raw: u64) -> TeksiloWindowId {
        TeksiloWindowId::new(raw)
    }

    #[test]
    fn no_window_has_no_most_recent() {
        assert_eq!(WindowRecency::default().most_recent(), None);
    }

    #[test]
    fn the_window_opened_last_is_the_most_recent() {
        let mut recency = WindowRecency::default();
        recency.touch(id(1));
        recency.touch(id(2));
        recency.touch(id(3));
        assert_eq!(recency.most_recent(), Some(id(3)));
    }

    #[test]
    fn focusing_an_older_window_makes_it_the_most_recent() {
        let mut recency = WindowRecency::default();
        recency.touch(id(1));
        recency.touch(id(2));
        recency.touch(id(1));
        assert_eq!(recency.most_recent(), Some(id(1)));
    }

    /// Closing the most recent window hands the role back to the window used
    /// before it, not to the oldest one or to an arbitrary one.
    #[test]
    fn closing_the_most_recent_window_falls_back_to_the_one_before() {
        let mut recency = WindowRecency::default();
        recency.touch(id(1));
        recency.touch(id(2));
        recency.touch(id(3));
        recency.touch(id(2));
        recency.forget(id(2));
        assert_eq!(recency.most_recent(), Some(id(3)));
        recency.forget(id(3));
        assert_eq!(recency.most_recent(), Some(id(1)));
        recency.forget(id(1));
        assert_eq!(recency.most_recent(), None);
    }

    #[test]
    fn stamps_order_the_windows_and_go_with_them() {
        let mut recency = WindowRecency::default();
        recency.touch(id(1));
        recency.touch(id(2));
        assert!(recency.stamp(id(2)) > recency.stamp(id(1)));
        recency.forget(id(2));
        assert_eq!(recency.stamp(id(2)), None);
    }

    #[test]
    fn forgetting_an_unknown_window_changes_nothing() {
        let mut recency = WindowRecency::default();
        recency.touch(id(1));
        recency.forget(id(9));
        assert_eq!(recency.most_recent(), Some(id(1)));
    }

    /// The answer does not depend on the hash order: many windows, touched in
    /// a scrambled order, always resolve to the last one touched.
    #[test]
    fn the_answer_is_the_last_touch_whatever_the_hash_order() {
        let mut recency = WindowRecency::default();
        for raw in 1..=64 {
            recency.touch(id(raw));
        }
        for raw in [17, 3, 64, 40, 9, 33] {
            recency.touch(id(raw));
            assert_eq!(recency.most_recent(), Some(id(raw)));
        }
    }
}
