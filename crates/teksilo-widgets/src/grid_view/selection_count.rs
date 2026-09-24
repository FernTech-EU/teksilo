// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Saying how many tiles are selected.
//!
//! The count used to be the grid's value, with the grid marked as a polite
//! live region so that a change would be announced. No platform ever said it.
//! Every AccessKit adapter announces a live node's *name*, and a `Grid` takes
//! its name from its label: only a `Role::Label` takes it from its value
//! (`accesskit_consumer-0.39.0` `node.rs:744-746`). The live setting did reach
//! something, though. `accesskit_consumer` hands a node's `live` down to every
//! descendant that sets none of its own (`node.rs:906-910`), so each named tile
//! that scrolled into the realized window was announced as it arrived, on all
//! three platforms.
//!
//! The grid is no longer live. The count is spoken through the tree's
//! announcer instead, by the user's action that changed it: a click, a key, an
//! assistive click on a tile, a marquee. An application that changes the
//! selection itself knows it has, and says so in its own words if it wants to.
//! The grid's value still carries the count, in the user's language, for a
//! screen reader that reports it with the grid.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_core::widget::EventContext;
use teksilo_data::SelectionModel;

/// How many tiles are selected, in the user's language: "1 élément
/// sélectionné", "3 items selected", "No item selected".
pub(crate) fn selection_count_words(count: usize) -> String {
    let count = i64::try_from(count).unwrap_or(i64::MAX);
    teksilo_i18n::tr_widget!(grid_view_selection_count(count = count))
        .resolve_now()
        .to_string()
}

/// Runs something the user did and, when it changed how many tiles are
/// selected, says the new count.
///
/// Clones share one voice. The grid wraps the key handler in it, and Space
/// wraps its own selection too, because that selection normally runs after
/// the handler has returned; when the focused tile is not realized it runs
/// there and then, inside the handler, and each wrapper would have said the
/// same count. Only the outermost call counts.
#[derive(Clone)]
pub(crate) struct SelectionCountVoice {
    selection: SelectionModel,
    counting: Rc<Cell<bool>>,
}

impl SelectionCountVoice {
    pub(crate) fn new(selection: SelectionModel) -> Self {
        Self {
            selection,
            counting: Rc::new(Cell::new(false)),
        }
    }

    /// Run `act`, then announce the count if `act` changed it. A move of the
    /// cursor in a single selection changes which tile is selected and not how
    /// many, and says nothing here: the tile the reader lands on says it is
    /// selected. Inside another call on the same voice, `act` just runs, and
    /// the outer call says what the two did together.
    pub(crate) fn around<R>(
        &self,
        ctx: &mut EventContext,
        act: impl FnOnce(&mut EventContext) -> R,
    ) -> R {
        if self.counting.replace(true) {
            return act(ctx);
        }
        // Handed back on the way out, however `act` leaves, so a handler that
        // unwinds does not leave the voice thinking it is still counting and
        // silence every later change.
        let _counting = Counting(&self.counting);
        let before = self.selection.count();
        let result = act(ctx);
        let after = self.selection.count();
        if after != before {
            ctx.announce(selection_count_words(after));
        }
        result
    }
}

/// Clears the voice's flag when the outermost `around` is left.
struct Counting<'a>(&'a Cell<bool>);

impl Drop for Counting<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}
