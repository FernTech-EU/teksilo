//! A tiny single-threaded (`!Send`) cooperative task executor, driven once per
//! event-loop turn by the [`on_loop_tick`](bastyde_app::BastydeAppBuilder::on_loop_tick)
//! hook in `bastyde-app`.
//!
//! ## Why hand-rolled
//!
//! The executor must (a) hold `!Send` futures that capture `Rc`-based `Signal`
//! handles, (b) be polled cooperatively from winit's `about_to_wait` without
//! ever blocking, and (c) be woken from *another* thread when a
//! [`spawn_blocking`](crate::spawn_blocking) worker finishes. Requirement (c)
//! is the crux: the wake may arrive off-thread, but the task queue is `Rc`
//! (`!Send`). We solve it with a single shared [`Waker`] (`Arc<ExecWaker>`,
//! `Send + Sync`) handed to every task's leaf futures. On wake it only sets an
//! atomic flag and nudges the event loop through the (`Send + Sync`)
//! [`AppEventPoster`] — it never touches the `!Send` queue. The main thread
//! then re-polls live tasks on the next tick. Polling every live task per wake
//! is `O(n)`, but `n` (concurrent UI async tasks) is tiny.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Wake, Waker};

use bastyde_core::{
    AppEventPoster, AsyncCompletionHandle, AsyncCompletionPayload, BastydeWindowId, EventContext,
};

/// Unit payload posted through the [`AppEventPoster`] to wake a sleeping event
/// loop when a task becomes runnable from another thread. `bastyde-app` does
/// not recognise the type — it falls through the `AppEvent::External` downcast
/// chain and is dropped — but the wake side effect (the loop runs one more
/// turn, calling the registered loop-tick) is exactly what we need.
struct AsyncWake;

/// Shared wake state. The [`Waker`] handed to every task's leaf futures is
/// built from an `Arc<ExecWaker>`; when a leaf wakes (possibly from a worker
/// thread) it sets `woken` and nudges the event loop.
struct ExecWaker {
    woken: AtomicBool,
    poster: OnceLock<Arc<dyn AppEventPoster>>,
}

impl Wake for ExecWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.woken.store(true, Ordering::SeqCst);
        if let Some(poster) = self.poster.get() {
            poster.post_external(Box::new(AsyncWake));
        }
    }
}

type BoxFuture = Pin<Box<dyn Future<Output = ()>>>;

struct Task {
    future: BoxFuture,
    cancelled: Rc<Cell<bool>>,
}

/// Handle to a spawned task. Dropping it cancels the task (the future is
/// dropped on the next tick); call [`detach`](TaskHandle::detach) to let the
/// task run to completion independently of the handle.
#[must_use = "dropping the TaskHandle cancels the task — call `.detach()` to let it keep running"]
pub struct TaskHandle {
    cancelled: Option<Rc<Cell<bool>>>,
}

impl TaskHandle {
    /// Let the task run to completion; dropping this handle no longer cancels
    /// it. The classic fire-and-forget terminator: `ctx.spawn_local(..).detach()`.
    pub fn detach(mut self) {
        self.cancelled = None;
    }
}

impl Drop for TaskHandle {
    fn drop(&mut self) {
        if let Some(flag) = &self.cancelled {
            flag.set(true);
        }
    }
}

struct ExecInner {
    tasks: RefCell<Vec<Task>>,
    /// Tasks spawned since the last `flush` — kept separate so a task may spawn
    /// another during its own poll without re-borrowing `tasks`.
    spawn_queue: RefCell<Vec<Task>>,
    wake: Arc<ExecWaker>,
    waker: Waker,
    poll_source: Rc<Cell<bool>>,
    completions: AsyncCompletionHandle,
    /// Re-entrancy guard for [`ExecInner::tick`].
    ticking: Cell<bool>,
}

/// RAII reset for `tick`'s re-entrancy guard — clears the flag on scope exit,
/// including while unwinding if a task poll panics.
struct TickGuard<'a>(&'a Cell<bool>);

impl Drop for TickGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

impl ExecInner {
    fn flush_spawns(&self) {
        let mut queued = self.spawn_queue.borrow_mut();
        if !queued.is_empty() {
            self.tasks.borrow_mut().append(&mut queued);
        }
    }

    fn spawn(&self, future: BoxFuture) -> TaskHandle {
        let cancelled = Rc::new(Cell::new(false));
        self.spawn_queue.borrow_mut().push(Task {
            future,
            cancelled: cancelled.clone(),
        });
        // A spawn always happens inside an event dispatch, and `about_to_wait`
        // (→ the loop tick) runs before the loop next sleeps, so the task gets
        // its first poll without an explicit proxy nudge.
        self.wake.woken.store(true, Ordering::SeqCst);
        TaskHandle {
            cancelled: Some(cancelled),
        }
    }

    fn tick(&self) -> bool {
        // Re-entrancy guard: only the event-loop hook should drive ticks. A
        // re-entrant call (e.g. a future's poll calling back in) would corrupt
        // the take/restore of `tasks`, so ignore it. The RAII reset keeps the
        // flag correct even if a task poll panics and unwinds.
        if self.ticking.get() {
            return false;
        }
        self.ticking.set(true);
        let _reset = TickGuard(&self.ticking);

        self.flush_spawns();
        // Nothing woke us since the last tick → idle. Clear the poll source so
        // the loop sleeps in `ControlFlow::Wait` until the next wake nudges it.
        if !self.wake.woken.swap(false, Ordering::SeqCst) {
            self.poll_source.set(false);
            return false;
        }

        let mut cx = Context::from_waker(&self.waker);
        // Take the live tasks out so a task may freely spawn/cancel during its
        // own poll without re-borrowing `tasks`.
        let taken = std::mem::take(&mut *self.tasks.borrow_mut());
        let mut survivors = Vec::with_capacity(taken.len());
        for mut task in taken {
            if task.cancelled.get() {
                continue; // dropped/detached-then-dropped — discard the future
            }
            match task.future.as_mut().poll(&mut cx) {
                Poll::Ready(()) => {} // done — drop the future
                Poll::Pending => survivors.push(task),
            }
        }
        *self.tasks.borrow_mut() = survivors;
        // Pull in tasks spawned during this poll so they run next turn.
        self.flush_spawns();

        // If a task re-woke synchronously (spawned another, yielded, …) keep
        // polling next turn via `ControlFlow::Poll`; otherwise the loop sleeps
        // until the next wake nudges it through the proxy.
        self.poll_source.set(self.wake.woken.load(Ordering::SeqCst));
        true
    }
}

/// Handle to the main-thread async runtime. Registered in app-state by
/// [`install_async`](crate::BastydeAppBuilderAsyncExt::install_async) and
/// reached from a handler via `ctx.spawn_local(...)`
/// ([`EventContextAsyncExt`](crate::EventContextAsyncExt)). `Clone` shares the
/// same executor (`Rc`); `!Send` — it only ever lives on the UI thread.
#[derive(Clone)]
pub struct AsyncRuntimeHandle {
    inner: Rc<ExecInner>,
}

impl Default for AsyncRuntimeHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl AsyncRuntimeHandle {
    pub fn new() -> Self {
        let wake = Arc::new(ExecWaker {
            woken: AtomicBool::new(false),
            poster: OnceLock::new(),
        });
        let waker = Waker::from(wake.clone());
        Self {
            inner: Rc::new(ExecInner {
                tasks: RefCell::new(Vec::new()),
                spawn_queue: RefCell::new(Vec::new()),
                wake,
                waker,
                poll_source: Rc::new(Cell::new(false)),
                completions: AsyncCompletionHandle::new(),
                ticking: Cell::new(false),
            }),
        }
    }

    /// The shared poll flag passed to `on_loop_tick`; set while the executor
    /// wants continuous polling (a task re-woke synchronously), cleared when
    /// idle so the loop can sleep.
    pub fn poll_source(&self) -> Rc<Cell<bool>> {
        self.inner.poll_source.clone()
    }

    /// The completion registry shared with `bastyde-app` so it can deliver
    /// [`spawn_local_with`](Self::spawn_local_with) results with a fresh
    /// [`EventContext`]. Registered in app-state under
    /// [`AsyncCompletionHandle`].
    pub fn completions(&self) -> AsyncCompletionHandle {
        self.inner.completions.clone()
    }

    /// Install the event-loop poster used as the cross-thread wake target and
    /// to post completions. Idempotent (set once). Called lazily on the first
    /// spawn from `ctx.poster()`.
    pub fn set_poster(&self, poster: Arc<dyn AppEventPoster>) {
        let _ = self.inner.wake.poster.set(poster);
    }

    /// Advance the executor by one turn: poll every live task that a wake made
    /// runnable, dropping completed/cancelled ones. Returns `true` if it
    /// polled tasks (the caller repaints). Normally driven by the `on_loop_tick`
    /// hook; exposed for headless drivers and tests.
    pub fn tick(&self) -> bool {
        self.inner.tick()
    }

    /// Spawn a `!Send` future on the main-thread executor. The future may
    /// capture and mutate `Signal`s and other `Rc` handles directly. Returns a
    /// [`TaskHandle`]: drop to cancel, `.detach()` to fire-and-forget.
    pub fn spawn_local(&self, future: impl Future<Output = ()> + 'static) -> TaskHandle {
        self.inner.spawn(Box::pin(future))
    }

    /// Spawn a future whose result is delivered to `on_complete` with a *fresh*
    /// [`EventContext`] bound to `window_id`'s tree — the supported way to run
    /// a one-shot ambient op (`open_window`, `send_intent`, …) after `await`.
    /// The future body itself runs handle-only (no `EventContext`).
    pub fn spawn_local_with<R: 'static>(
        &self,
        window_id: BastydeWindowId,
        future: impl Future<Output = R> + 'static,
        on_complete: impl FnOnce(R, &mut EventContext) + 'static,
    ) -> TaskHandle {
        // Capture only the (separate-`Rc`) completion registry and the
        // (`Arc`) poster, NOT the executor `Rc`, so the wrapper future does
        // not form a reference cycle with `ExecInner`.
        let completions = self.inner.completions.clone();
        let poster = self.inner.wake.poster.get().cloned();
        let wrapper = async move {
            let result = future.await;
            let callback: Box<dyn FnOnce(&mut EventContext)> =
                Box::new(move |ctx| on_complete(result, ctx));
            let id = completions.register(window_id, callback);
            if let Some(poster) = &poster {
                poster.post_external(Box::new(AsyncCompletionPayload { id, window_id }));
            }
        };
        self.inner.spawn(Box::pin(wrapper))
    }
}
