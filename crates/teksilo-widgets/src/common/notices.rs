// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a text surface tells the application's `on_text_inserted` and
//! `on_change` callbacks: queued where it happens, under the surface's borrow,
//! and delivered once the borrow is gone.
//!
//! An edit is reported from deep inside the surface (the frame tick's drain, a
//! key handler, a paste) while its state is borrowed. A callback run there
//! could not read the surface through its handle, let alone edit it: the borrow
//! it needed was held by its own caller. So the report is queued, and
//! [`deliver`] calls the callbacks afterwards, from the event handler, frame
//! tick or handle method that produced it, with nothing borrowed: snapshot,
//! release, notify, as `Signal` does. Queuing asks for a frame, whose tick
//! delivers whatever the code that queued it did not.
//!
//! **A callback's own edits come back to it after it returns, in order.** Only
//! the outermost [`deliver`] runs callbacks: an edit made through the handle
//! inside a callback queues its report, and the loop takes it once the
//! callback has returned. The stack does not grow with a chain of edits, and
//! reports reach the application in the order the edits were made. A chain
//! that never ends (a callback answering every insertion with another) is a
//! bug in the application; past [`CHAIN_LIMIT`] reports in one delivery a
//! debug build panics naming it, as `Signal` does for a feedback loop between
//! observers, and a release build leaves the rest for the next frame, so the
//! window keeps drawing.
//!
//! Shared by `RichTextEditor`, `TextInputField` and `CodeEditor`.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use crate::EditSource;

/// More reports than any edit and its legitimate follow-ups produce in one
/// delivery (an emoticon replacement is two), and few enough that a runaway
/// chain is caught at once. The number `Signal` allows for nested
/// notifications.
pub(crate) const CHAIN_LIMIT: usize = 256;

/// One report waiting for its callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Notice {
    /// For `on_text_inserted`: the route the text came down and how many
    /// characters it was.
    TextInserted(EditSource, usize),
    /// For `on_change`: a batch of user edits was drained.
    Changed,
}

/// The reports waiting for [`deliver`], and whether a delivery is running.
///
/// Interior mutability because reports are queued through a shared borrow of
/// the surface's state (an insertion site holds `&State`).
#[derive(Debug, Default)]
pub(crate) struct Notices {
    queue: RefCell<VecDeque<Notice>>,
    delivering: Cell<bool>,
}

impl Notices {
    /// Queue `notice` and ask for a frame through `frame_request`.
    pub(crate) fn push(&self, notice: Notice, frame_request: Option<&Rc<Cell<bool>>>) {
        self.queue.borrow_mut().push_back(notice);
        if let Some(request) = frame_request {
            request.set(true);
        }
    }

    fn pop(&self) -> Option<Notice> {
        self.queue.borrow_mut().pop_front()
    }

    fn is_empty(&self) -> bool {
        self.queue.borrow().is_empty()
    }
}

/// A surface state that queues [`Notice`]s.
pub(crate) trait NoticeSource {
    /// The widget's name, for the panic that stops a runaway chain.
    const WIDGET: &'static str;
    fn notices(&self) -> &Notices;
    fn frame_request(&self) -> Option<&Rc<Cell<bool>>>;
    fn on_text_inserted(&self) -> Option<Rc<dyn Fn(EditSource, usize)>>;
    fn on_change(&self) -> Option<Rc<dyn Fn()>>;
}

/// Clears the delivering flag when [`deliver`] returns, and when a callback
/// panics through it, so the next delivery is not refused.
struct Delivering<'a, S: NoticeSource>(&'a RefCell<S>);

impl<S: NoticeSource> Drop for Delivering<'_, S> {
    fn drop(&mut self) {
        if let Ok(st) = self.0.try_borrow() {
            st.notices().delivering.set(false);
        }
    }
}

/// Run the callbacks for every queued report, oldest first, with no borrow of
/// the surface's state held while one runs.
///
/// Call it where the borrow that queued the reports has ended: after the frame
/// tick, an event handler or a handle method. Does nothing when the state is
/// borrowed (the caller is still inside the surface; the next delivery takes
/// the reports) or when a delivery is already running further up the stack (it
/// takes them once the current callback returns).
pub(crate) fn deliver<S: NoticeSource>(state: &RefCell<S>) {
    {
        let Ok(st) = state.try_borrow() else {
            return;
        };
        if st.notices().delivering.replace(true) {
            return;
        }
    }
    let _delivering = Delivering(state);
    for delivered in 0.. {
        let (notice, on_text_inserted, on_change) = {
            let Ok(st) = state.try_borrow() else {
                return;
            };
            if delivered == CHAIN_LIMIT && !st.notices().is_empty() {
                if cfg!(debug_assertions) {
                    drop(st);
                    panic!(
                        "{}: {CHAIN_LIMIT} on_text_inserted / on_change reports in one \
                         delivery, almost certainly a callback that edits the text on every \
                         report it gets (each edit is reported again). Make the edit \
                         conditional on what the callback reads, so that its own edit finds \
                         nothing left to do.",
                        S::WIDGET
                    );
                }
                if let Some(request) = st.frame_request() {
                    request.set(true);
                }
                return;
            }
            let Some(notice) = st.notices().pop() else {
                return;
            };
            (notice, st.on_text_inserted(), st.on_change())
        };
        match notice {
            Notice::TextInserted(source, chars) => {
                if let Some(callback) = on_text_inserted {
                    callback(source, chars);
                }
            }
            Notice::Changed => {
                if let Some(callback) = on_change {
                    callback();
                }
            }
        }
    }
}
