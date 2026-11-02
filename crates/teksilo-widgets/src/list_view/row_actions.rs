// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The state of a [`ListView`](super::ListView) in
//! [`RowKeyboard::Actions`](crate::RowKeyboard::Actions): which row the user
//! has stepped into, and what keeps focus there across the rebuilds a list
//! lives in.
//!
//! One `Rc` shared by the root (keys, focus, data changes) and the body pane
//! (row wrappers, rebuilds). A view in `Toggle` mode has none, so nothing here
//! runs for it.
//!
//! # Why the state is not just "focus is inside row i"
//!
//! The pane rebuilds on every data change (an `ItemUpdated` included) and on
//! every window change, and the row wrappers rebuild on a selection flip. Each
//! rebuild destroys the focused control, which writes `false` into the row's
//! `focus_within` and the view's focus signal before the new row exists. So a
//! `false` edge proves nothing, and the mode is left only by explicit exits:
//! `Escape`, focus landing on the view itself, the entered row being removed,
//! and focus leaving the list (which turns `engaged` off, and only the next
//! arrival in a row turns it on again).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use teksilo_core::signal::Signal;
use teksilo_core::widget_id::WidgetId;
use teksilo_data::DataChange;

/// One realized row, as the pane last built it.
#[derive(Clone)]
pub(crate) struct RowSlot {
    /// Model index.
    pub(crate) index: usize,
    /// The row wrapper (the `ListBoxOption` node).
    pub(crate) wrapper: WidgetId,
    /// Whether a strict descendant of the wrapper holds focus.
    pub(crate) inside: Signal<bool>,
    /// The row's enabled controls in Tab order, as of the wrapper's last
    /// build.
    pub(crate) controls: Rc<RefCell<Vec<WidgetId>>>,
}

/// Action-mode state of one `ListView`. See the module docs.
pub(crate) struct RowActions {
    /// The model index of the row the user stepped into.
    pub(crate) entered: Signal<Option<usize>>,
    /// Whether focus is (or is about to be) inside the entered row. A row's
    /// controls are Tab stops only while this holds, so a row the user left
    /// with the mouse does not leak its controls into the window's Tab order.
    pub(crate) engaged: Signal<bool>,
    /// Whether focus is on the view node itself.
    pub(crate) focus_on_view: Cell<bool>,
    /// Whether focus was inside the entered row when the pane was last asked
    /// to rebuild. Read by that rebuild, after the destruction it causes has
    /// already cleared every focus signal.
    pub(crate) inside_at_bump: Cell<bool>,
    /// A data change moved the entered row to another index since the pane's
    /// last build.
    pub(crate) shifted: Cell<bool>,
    /// A data change removed the entered row while focus was inside it.
    pub(crate) refocus_view: Cell<bool>,
    /// The entered row left the realized window while focus was in it; focus
    /// waits on the view and goes back in when the row is realized again, if
    /// it is still on the view then.
    pub(crate) resume_on_realize: Cell<bool>,
    /// Set around the focus moves the pane makes when a scroll takes the
    /// entered row out of the window or brings it back: the move to the view
    /// must not end the mode, and neither move may reveal the cursor's row,
    /// which would scroll the user back to where they scrolled from.
    pub(crate) keep_on_view_focus: Cell<bool>,
    /// The first model index the pane realized in its last build.
    pub(crate) last_start: Cell<Option<usize>>,
    /// The realized rows of the pane's last build.
    pub(crate) rows: RefCell<Vec<RowSlot>>,
    /// The position, among its row's controls, of the control focus was last
    /// in. A rebuild that puts focus back explicitly (the row moved, the
    /// window moved, the row came back) puts it on the same control.
    pub(crate) ordinal: Rc<Cell<usize>>,
}

impl RowSlot {
    /// Whether the row holds a control focus can enter.
    pub(crate) fn has_control(&self) -> bool {
        !self.controls.borrow().is_empty()
    }

    /// The control at `ordinal`, else the first one.
    pub(crate) fn control(&self, ordinal: usize) -> Option<WidgetId> {
        let controls = self.controls.borrow();
        controls.get(ordinal).or(controls.first()).copied()
    }
}

impl RowActions {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self {
            entered: Signal::new(None),
            engaged: Signal::new(false),
            focus_on_view: Cell::new(false),
            inside_at_bump: Cell::new(false),
            shifted: Cell::new(false),
            refocus_view: Cell::new(false),
            resume_on_realize: Cell::new(false),
            keep_on_view_focus: Cell::new(false),
            last_start: Cell::new(None),
            rows: RefCell::new(Vec::new()),
            ordinal: Rc::new(Cell::new(0)),
        })
    }

    /// Whether focus is in the entered row now.
    pub(crate) fn inside_now(&self) -> bool {
        self.entered.get().is_some() && self.engaged.get() && !self.focus_on_view.get()
    }

    /// Leave action mode.
    pub(crate) fn exit(&self) {
        if self.entered.get().is_some() {
            self.entered.set(None);
        }
        if self.engaged.get() {
            self.engaged.set(false);
        }
        self.resume_on_realize.set(false);
    }

    /// Enter row `index`, whose controls focus is about to reach.
    pub(crate) fn enter(&self, index: usize) {
        if self.entered.get() != Some(index) {
            self.entered.set(Some(index));
        }
        if !self.engaged.get() {
            self.engaged.set(true);
        }
        self.resume_on_realize.set(false);
    }

    /// The realized row of model index `index`, if any.
    pub(crate) fn slot(&self, index: usize) -> Option<RowSlot> {
        self.rows
            .borrow()
            .iter()
            .find(|slot| slot.index == index)
            .cloned()
    }

    /// Follow the entered row through a model change, as the keyboard cursor
    /// does. A row that is gone ends the mode, and focus goes to the view if
    /// it was in that row.
    pub(crate) fn on_data_change(&self, change: &DataChange) {
        let Some(before) = self.entered.get() else {
            return;
        };
        match teksilo_data::data_change::adjust_single_index_for_change(before, change) {
            Some(after) if after == before => {}
            Some(after) => {
                self.entered.set(Some(after));
                self.shifted.set(true);
            }
            None => {
                self.refocus_view.set(self.inside_now());
                self.exit();
            }
        }
    }
}
