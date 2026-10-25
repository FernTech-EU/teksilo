// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A close guard's answer, given later: [`CloseDecision`].
//!
//! A guard that has to ask the user (a "Save changes?" dialog) cannot
//! answer the close on the spot. It returns
//! [`CloseResponse::Defer`](super::CloseResponse::Defer) with a
//! `CloseDecision`, and whoever learns the answer (the dialog's result
//! callback, an async completion, another thread) calls
//! [`proceed`](CloseDecision::proceed) or [`cancel`](CloseDecision::cancel)
//! on a clone of it.

use std::sync::{Arc, Mutex};

/// How a deferred close was answered. Read by the application through a
/// [`CloseDecisionReceipt`].
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAnswer {
    /// [`CloseDecision::proceed`]: the close goes ahead.
    Proceed,
    /// [`CloseDecision::cancel`]: the close is abandoned.
    Cancel,
    /// Every clone of the decision was dropped with no answer given. Taken
    /// as a cancel.
    Dropped,
}

/// Called when the decision is answered, from whatever thread answers it.
type Wake = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct Slot {
    answer: Option<CloseAnswer>,
    wake: Option<Wake>,
}

/// Record `answer` unless one was given already, and wake whoever waits.
fn answer(slot: &Mutex<Slot>, answer: CloseAnswer) {
    let wake = {
        let mut slot = slot.lock().unwrap_or_else(|poison| poison.into_inner());
        if slot.answer.is_some() {
            return;
        }
        slot.answer = Some(answer);
        slot.wake.clone()
    };
    if let Some(wake) = wake {
        wake();
    }
}

/// What the clones of a [`CloseDecision`] share: the last one dropped
/// answers [`CloseAnswer::Dropped`] if nothing answered before.
struct Holder(Arc<Mutex<Slot>>);

impl Drop for Holder {
    fn drop(&mut self) {
        answer(&self.0, CloseAnswer::Dropped);
    }
}

/// The answer to a close a guard deferred: returned with
/// [`CloseResponse::Defer`](super::CloseResponse::Defer), then answered
/// once with [`proceed`](Self::proceed) or [`cancel`](Self::cancel).
///
/// The close it answers is the one the guard was asked about: the
/// window's own close (its close button, `close_window`), or the window's
/// part in a quit, in which case the answer also resumes or abandons the
/// quit. `proceed` closes the window without asking its guard again;
/// `cancel` leaves it open and, for a quit, abandons the whole quit.
///
/// Cloneable and `Send`, so a clone can go into a dialog's result
/// callback, an async task or another thread. The first answer counts and
/// later ones are ignored. Dropping every clone without answering cancels
/// the close, and a debug build says so: a guard that defers owes an
/// answer. Answering a decision whose close no longer waits (its window
/// was closed another way, or a forced quit closed it) does nothing.
///
/// ```ignore
/// .on_close_requested(move |ctx| {
///     if !dirty.get() {
///         return CloseResponse::Close;
///     }
///     let decision = CloseDecision::new();
///     let answer = decision.clone();
///     ctx.present_message_box(
///         MessageBox::question(lit!("Save changes?"))
///             .buttons(MessageBoxButtons::SaveDiscardCancel)
///             .on_result(move |result, _ctx| match result.button {
///                 StandardButton::Save => { save(); answer.proceed(); }
///                 StandardButton::Discard => answer.proceed(),
///                 _ => answer.cancel(),
///             }),
///     );
///     CloseResponse::Defer(decision)
/// })
/// ```
#[derive(Clone)]
pub struct CloseDecision(Arc<Holder>);

impl CloseDecision {
    /// A decision with no answer yet.
    pub fn new() -> Self {
        Self(Arc::new(Holder(Arc::new(Mutex::new(Slot::default())))))
    }

    /// Let the close go ahead: the window closes without its guard being
    /// asked again, and a quit waiting on it goes on.
    pub fn proceed(&self) {
        answer(&self.0.0, CloseAnswer::Proceed);
    }

    /// Abandon the close: the window stays open, and a quit waiting on it
    /// is abandoned.
    pub fn cancel(&self) {
        answer(&self.0.0, CloseAnswer::Cancel);
    }

    /// The application's view of this decision, which does not keep it
    /// alive.
    #[doc(hidden)]
    pub fn receipt(&self) -> CloseDecisionReceipt {
        CloseDecisionReceipt(self.0.0.clone())
    }
}

impl Default for CloseDecision {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for CloseDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("CloseDecision")
            .field(&self.receipt().answer())
            .finish()
    }
}

/// The application's side of a [`CloseDecision`]: reads the answer and
/// learns when it is given, without keeping the decision alive, so that
/// dropping every clone the guard handed out still answers
/// [`CloseAnswer::Dropped`].
#[doc(hidden)]
#[derive(Clone)]
pub struct CloseDecisionReceipt(Arc<Mutex<Slot>>);

impl CloseDecisionReceipt {
    /// The answer given, if any.
    pub fn answer(&self) -> Option<CloseAnswer> {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .answer
    }

    /// Call `wake` when the answer is given, from the thread that gives
    /// it. Replaces an earlier `wake`; not called for an answer already
    /// given.
    pub fn on_answer(&self, wake: Arc<dyn Fn() + Send + Sync>) {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .wake = Some(wake);
    }
}

impl std::fmt::Debug for CloseDecisionReceipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("CloseDecisionReceipt")
            .field(&self.answer())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn counting_wake() -> (Arc<AtomicUsize>, Wake) {
        let count = Arc::new(AtomicUsize::new(0));
        let wake: Wake = {
            let count = count.clone();
            Arc::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
            })
        };
        (count, wake)
    }

    #[test]
    fn unanswered_until_answered_and_the_first_answer_counts() {
        let decision = CloseDecision::new();
        let receipt = decision.receipt();
        assert_eq!(receipt.answer(), None);
        decision.clone().proceed();
        assert_eq!(receipt.answer(), Some(CloseAnswer::Proceed));
        decision.cancel();
        assert_eq!(
            receipt.answer(),
            Some(CloseAnswer::Proceed),
            "a later answer is ignored"
        );
    }

    #[test]
    fn dropping_every_clone_unanswered_answers_dropped() {
        let decision = CloseDecision::new();
        let receipt = decision.receipt();
        let clone = decision.clone();
        drop(decision);
        assert_eq!(receipt.answer(), None, "a clone is still alive");
        drop(clone);
        assert_eq!(receipt.answer(), Some(CloseAnswer::Dropped));
    }

    #[test]
    fn dropping_an_answered_decision_keeps_its_answer() {
        let decision = CloseDecision::new();
        let receipt = decision.receipt();
        decision.cancel();
        drop(decision);
        assert_eq!(receipt.answer(), Some(CloseAnswer::Cancel));
    }

    #[test]
    fn the_receipt_does_not_keep_the_decision_alive() {
        let decision = CloseDecision::new();
        let receipt = decision.receipt();
        let _second_receipt = receipt.clone();
        drop(decision);
        assert_eq!(receipt.answer(), Some(CloseAnswer::Dropped));
    }

    #[test]
    fn the_wake_runs_once_for_the_answer_and_not_for_an_ignored_one() {
        let decision = CloseDecision::new();
        let (count, wake) = counting_wake();
        decision.receipt().on_answer(wake);
        decision.proceed();
        decision.cancel();
        drop(decision);
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_wake_runs_for_a_drop_too() {
        let decision = CloseDecision::new();
        let (count, wake) = counting_wake();
        decision.receipt().on_answer(wake);
        drop(decision);
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn answered_from_another_thread() {
        let decision = CloseDecision::new();
        let receipt = decision.receipt();
        let answer = decision.clone();
        std::thread::spawn(move || answer.proceed())
            .join()
            .expect("the answering thread");
        assert_eq!(receipt.answer(), Some(CloseAnswer::Proceed));
    }
}
