// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The deferred "scroll this row into view" request behind the row views'
//! handles ([`ListViewHandle`](crate::ListViewHandle),
//! [`TreeViewHandle`](crate::TreeViewHandle),
//! [`TableViewHandle`](crate::TableViewHandle),
//! [`TreeTableViewHandle`](crate::TreeTableViewHandle)).
//!
//! ## Why the offset is not computed when the request is made
//!
//! Where a row sits is a layout fact. At the moment an application asks for
//! it, three things can be stale:
//!
//! * **the scroll range** — `max_scroll_y` is written by the view's
//!   `place_children`, so before the first layout it is `0` and every target
//!   clamps to the top, and after a same-turn insert it still describes the
//!   shorter list;
//! * **the row offsets** — a model change that lands in the same turn (rows
//!   inserted above, a tree node expanded) has invalidated the measured
//!   heights from the change onward, and with auto-measured rows those rows
//!   only learn their height when they are realized;
//! * **the viewport** — the cached height is a placeholder until the view is
//!   first placed.
//!
//! So a request only records *what* to reveal ([`RowReveal::request`]) and
//! bumps a signal the view binds at `Relayout`. The body pane — which owns the
//! measure pass and the scroll anchoring — applies it in its
//! `place_children`, after both have run ([`RowReveal::apply`]).
//!
//! ## Why it may take more than one pass
//!
//! With auto-measured rows, the target row's height is a guess until the pane
//! realizes and measures it, which needs the scroll to have moved there first.
//! The request therefore stays pending while the row is unmeasured and the
//! application moved the scroll: that move realizes the row (the pane's
//! buffer-exit rebuild), the next pass measures it, and the request is applied
//! again against real heights. Rows measured on the way above the target are
//! absorbed by the scroll anchoring, which keeps the target where the previous
//! pass put it. Once the row is measured — or the scroll did not need to
//! move — the request is done. A later request replaces an earlier pending
//! one.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;

/// Where a revealed row should end up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RevealAlign {
    /// Aligned to the top of the viewport, as far as the scroll range allows
    /// (`scroll_to_index` / `scroll_to_row`).
    Top,
    /// Moved the minimum distance that makes it fully visible, and not at all
    /// when it already is (`ensure_index_visible` / `ensure_row_visible`).
    Nearest,
}

/// One pending reveal: the row (a model index, or a flat visible index for
/// the tree views) and where it should land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PendingReveal {
    pub(crate) index: usize,
    pub(crate) align: RevealAlign,
}

/// Shared state between a view's handle and the view it drives. Created with
/// the view, so a handle taken before mount and every rebuilt body pane talk
/// to the same instance.
#[derive(Debug)]
pub(crate) struct RowReveal {
    pending: Cell<Option<PendingReveal>>,
    /// Bumped by every request. The root and the body pane bind it at
    /// `Relayout`, so a request made against a laid-out, clean tree still
    /// gets a pass to be applied in.
    request: Signal<u64>,
}

/// The handle form of [`RowReveal`]: what the views and their handles hold.
pub(crate) type SharedRowReveal = Rc<RowReveal>;

impl RowReveal {
    pub(crate) fn new() -> SharedRowReveal {
        Rc::new(Self {
            pending: Cell::new(None),
            request: Signal::new(0),
        })
    }

    /// Record a reveal for the next layout pass, replacing any pending one,
    /// and ask for that pass.
    pub(crate) fn request(&self, index: usize, align: RevealAlign) {
        self.pending.set(Some(PendingReveal { index, align }));
        self.request.set(self.request.get().wrapping_add(1));
    }

    /// The reveal waiting to be applied, if any.
    pub(crate) fn pending(&self) -> Option<PendingReveal> {
        self.pending.get()
    }

    /// Drop the pending reveal — for a view that has no rows to reveal.
    pub(crate) fn clear(&self) {
        self.pending.set(None);
    }

    /// Bind the request signal on the widget being built, at `Relayout`.
    pub(crate) fn bind(&self, ctx: &mut BuildContext) {
        self.request.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );
    }

    /// Apply the pending reveal, if any. Called by a body pane's
    /// `place_children` after its measure pass and scroll anchoring.
    ///
    /// * `count` — the number of rows; an empty view drops the request, and
    ///   an index past the end reveals the last row.
    /// * `is_measured(index)` — whether the row's height is known: always in
    ///   the uniform and exact-callback modes, and in the auto-measure mode
    ///   only when the pane realized (so measured) it this pass.
    /// * `target(index, align, scroll)` — the scroll offset that reveals the
    ///   row, clamped to the scroll range as the view's own builder method
    ///   clamps it.
    pub(crate) fn apply(
        &self,
        scroll_y: &Signal<f32>,
        count: usize,
        is_measured: impl FnOnce(usize) -> bool,
        target: impl FnOnce(usize, RevealAlign, f32) -> f32,
    ) {
        let Some(pending) = self.pending.get() else {
            return;
        };
        if count == 0 {
            self.pending.set(None);
            return;
        }
        let index = pending.index.min(count - 1);
        let current = scroll_y.get();
        let next = target(index, pending.align, current);
        let moved = (next - current).abs() > 0.01;
        if moved {
            // From `place_children`: the pane reads the new offset for its own
            // placement right after this, and the root and the pane's
            // buffer-exit observer pick it up next pass.
            scroll_y.set(next);
        }
        // Keep it only while the row's height is still a guess AND this pass
        // moved the scroll: the move realizes the row, whose next pass
        // measures it. A pass that did not move cannot realize anything new,
        // so keeping the request then could only spin.
        if !moved || is_measured(index) {
            self.pending.set(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_later_request_replaces_an_earlier_one() {
        let reveal = RowReveal::new();
        reveal.request(3, RevealAlign::Top);
        reveal.request(7, RevealAlign::Nearest);
        assert_eq!(
            reveal.pending(),
            Some(PendingReveal {
                index: 7,
                align: RevealAlign::Nearest
            })
        );
    }

    #[test]
    fn an_unmeasured_row_stays_pending_only_while_the_scroll_moves() {
        let reveal = RowReveal::new();
        let scroll = Signal::new(0.0_f32);
        reveal.request(10, RevealAlign::Top);
        reveal.apply(&scroll, 20, |_| false, |_, _, _| 200.0);
        assert_eq!(scroll.get(), 200.0);
        assert!(reveal.pending().is_some(), "unmeasured and moved: kept");
        reveal.apply(&scroll, 20, |_| false, |_, _, _| 200.0);
        assert!(reveal.pending().is_none(), "nothing moved: done");
    }

    #[test]
    fn an_index_past_the_end_reveals_the_last_row_and_an_empty_view_drops_it() {
        let reveal = RowReveal::new();
        let scroll = Signal::new(0.0_f32);
        reveal.request(99, RevealAlign::Top);
        let mut seen = None;
        reveal.apply(
            &scroll,
            5,
            |_| true,
            |i, _, s| {
                seen = Some(i);
                s
            },
        );
        assert_eq!(seen, Some(4));

        reveal.request(2, RevealAlign::Top);
        reveal.apply(&scroll, 0, |_| true, |_, _, _| unreachable!());
        assert!(reveal.pending().is_none());
    }
}
