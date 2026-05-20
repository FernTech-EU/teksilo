//! Generic, runtime-agnostic delivery of a one-shot main-thread callback that
//! runs with a *fresh* [`EventContext`] bound to a window's tree.
//!
//! This is the plumbing the optional `bastyde-async` crate uses to implement
//! `spawn_local_with`: a future runs on the main-thread executor, and when it
//! completes its result is handed to a callback that needs ambient context
//! operations (`open_window`, `send_intent`, …). Those operations require an
//! [`EventContext`], which only exists *during* event dispatch — so the
//! callback is *registered* here (keyed by an id and the originating window)
//! and *delivered* later by `bastyde-app`, which routes an
//! [`AsyncCompletionPayload`] to the window's tree and calls
//! [`AsyncCompletionHandle::deliver`] inside a freshly-minted context.
//!
//! It mirrors the file-dialog result-delivery pattern, but uses only
//! bastyde-core types so a crate layered *above* `bastyde-app` (like
//! `bastyde-async`) can register callbacks without forcing `bastyde-app` to
//! depend on it (which would be a dependency cycle). There is no async,
//! future, or runtime type here — just a callback registry and a `Send`
//! payload.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::widget::EventContext;
use crate::window::BastydeWindowId;

type CompletionCallback = Box<dyn FnOnce(&mut EventContext)>;

struct Pending {
    window_id: BastydeWindowId,
    callback: CompletionCallback,
}

struct CompletionState {
    next_id: u64,
    pending: HashMap<u64, Pending>,
}

/// Main-thread registry of pending async completions. `Clone` shares the same
/// inner state (`Rc`), so the executor and the app event loop both hold a
/// handle to one registry. `!Send` by construction — completions only ever run
/// on the UI thread.
#[derive(Clone)]
pub struct AsyncCompletionHandle {
    inner: Rc<RefCell<CompletionState>>,
}

impl Default for AsyncCompletionHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl AsyncCompletionHandle {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(CompletionState {
                next_id: 0,
                pending: HashMap::new(),
            })),
        }
    }

    /// Register a callback to run later with a fresh [`EventContext`] on the
    /// tree of `window_id`. Returns the id to place in an
    /// [`AsyncCompletionPayload`].
    pub fn register(&self, window_id: BastydeWindowId, callback: CompletionCallback) -> u64 {
        let mut state = self.inner.borrow_mut();
        let id = state.next_id;
        state.next_id = state.next_id.wrapping_add(1);
        state.pending.insert(id, Pending { window_id, callback });
        id
    }

    /// Invoke and remove the completion registered under `id`, if its target
    /// window still matches `window_id`. Called by `bastyde-app` from inside
    /// [`WidgetTree::run_with_event_context`](crate::WidgetTree::run_with_event_context).
    /// A no-op if the entry was already purged (e.g. the window closed) — the
    /// use-after-free guard.
    pub fn deliver(&self, id: u64, window_id: BastydeWindowId, ctx: &mut EventContext) {
        let entry = self.inner.borrow_mut().pending.remove(&id);
        if let Some(pending) = entry
            && pending.window_id == window_id
        {
            (pending.callback)(ctx);
        }
    }

    /// Drop every pending completion targeting `window_id`. Called when a
    /// window closes so a late-arriving completion never touches a torn-down
    /// tree.
    pub fn purge_window(&self, window_id: BastydeWindowId) {
        self.inner
            .borrow_mut()
            .pending
            .retain(|_, p| p.window_id != window_id);
    }

    /// Number of pending completions (diagnostics / tests).
    pub fn pending_len(&self) -> usize {
        self.inner.borrow().pending.len()
    }
}

/// `Send` payload posted through [`AppEventPoster`](crate::AppEventPoster) when
/// an async task completes. `bastyde-app` downcasts it, routes to the target
/// window's tree, and calls [`AsyncCompletionHandle::deliver`] with a fresh
/// context. Carries only ids — the (`!Send`) callback stays in the registry.
#[derive(Debug, Clone, Copy)]
pub struct AsyncCompletionPayload {
    pub id: u64,
    pub window_id: BastydeWindowId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget_tree::WidgetTree;
    use crate::window::NoopWindowOps;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn deliver_runs_callback_with_fresh_context() {
        let handle = AsyncCompletionHandle::new();
        let win = BastydeWindowId::new(1);
        let ran = Rc::new(Cell::new(false));
        let flag = ran.clone();
        let id = handle.register(win, Box::new(move |_ctx| flag.set(true)));
        assert_eq!(handle.pending_len(), 1);

        let mut tree = WidgetTree::new();
        tree.run_with_event_context(&mut NoopWindowOps, |ctx| handle.deliver(id, win, ctx));

        assert!(ran.get(), "callback must run with the fresh context");
        assert_eq!(handle.pending_len(), 0, "delivered completion must be removed");
    }

    #[test]
    fn purge_window_drops_only_that_windows_completions() {
        let handle = AsyncCompletionHandle::new();
        let win = BastydeWindowId::new(7);
        handle.register(win, Box::new(|_ctx| {}));
        handle.register(win, Box::new(|_ctx| {}));
        handle.register(BastydeWindowId::new(8), Box::new(|_ctx| {}));
        assert_eq!(handle.pending_len(), 3);

        handle.purge_window(win);
        assert_eq!(handle.pending_len(), 1, "only the other window's completion survives");
    }

    #[test]
    fn deliver_to_mismatched_window_does_not_run() {
        let handle = AsyncCompletionHandle::new();
        let win = BastydeWindowId::new(3);
        let ran = Rc::new(Cell::new(false));
        let flag = ran.clone();
        let id = handle.register(win, Box::new(move |_ctx| flag.set(true)));

        let mut tree = WidgetTree::new();
        tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
            handle.deliver(id, BastydeWindowId::new(999), ctx)
        });
        assert!(!ran.get(), "a window-mismatched delivery must not run the callback");
    }
}
