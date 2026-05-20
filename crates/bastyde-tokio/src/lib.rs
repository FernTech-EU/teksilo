//! # bastyde-tokio — Tokio reactor for Bastyde's async executor
//!
//! Thin adapter over [`bastyde-async`](bastyde_async): it installs the same
//! main-thread executor, but wraps each executor tick in a
//! [`tokio::runtime::Runtime`] context so **native Tokio futures** (timers,
//! sockets, `reqwest`, `sqlx`, …) can be `.await`ed directly inside
//! `ctx.spawn_local(...)` bodies.
//!
//! ```ignore
//! use bastyde_tokio::BastydeAppBuilderTokioExt;
//! BastydeAppBuilder::new().install_async_tokio() /* ... */ .run();
//!
//! // inside a handler:
//! ctx.spawn_local(async move {
//!     tokio::time::sleep(std::time::Duration::from_secs(1)).await;
//!     let body = reqwest::get(url).await?.text().await?;
//!     status.set(body);              // resume on the UI thread → set a Signal
//! })
//! .detach();
//! ```
//!
//! ## How it works
//!
//! The executor polls `!Send` futures on the UI thread. A multi-thread Tokio
//! runtime runs the reactor / timer driver on background threads. By entering
//! the runtime context (`runtime.enter()`) around each tick, a Tokio leaf
//! future polled on the UI thread registers with that background driver — and
//! crucially registers *our* executor's `Waker`. When the timer/socket is
//! ready, the background driver wakes our `Waker`, which nudges the event loop
//! to tick again. No blocking, no second event loop.

use std::future::Future;
use std::sync::Arc;

use bastyde_app::BastydeAppBuilder;
use bastyde_async::AsyncRuntimeHandle;

// Re-export the spawn surface so `use bastyde_tokio::*;` is enough for crate
// users that don't go through the `bastyde` umbrella prelude.
pub use bastyde_async::{EventContextAsyncExt, TaskHandle, spawn_blocking};

/// Shared background Tokio runtime, registered in app-state by
/// [`install_async_tokio`](BastydeAppBuilderTokioExt::install_async_tokio).
/// Fetch with `ctx.app_state::<TokioHandle>()` to spawn `Send` tasks on the
/// background runtime or obtain a [`tokio::runtime::Handle`]. `Clone` shares
/// the same runtime (`Arc`).
#[derive(Clone)]
pub struct TokioHandle {
    runtime: Arc<tokio::runtime::Runtime>,
}

impl TokioHandle {
    /// A cloneable [`tokio::runtime::Handle`] to the background runtime.
    pub fn handle(&self) -> tokio::runtime::Handle {
        self.runtime.handle().clone()
    }

    /// Spawn a `Send` future on the background runtime (off the UI thread).
    /// For `!Send` UI work that touches `Signal`s, use `ctx.spawn_local(...)`.
    pub fn spawn<F>(&self, future: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.runtime.spawn(future)
    }
}

/// Adds [`install_async_tokio`](BastydeAppBuilderTokioExt::install_async_tokio)
/// to the app builder.
pub trait BastydeAppBuilderTokioExt {
    /// Install the main-thread executor with a background Tokio runtime as its
    /// reactor. Registers the [`AsyncRuntimeHandle`], its completion registry,
    /// and a [`TokioHandle`] in app-state, and drives the executor inside the
    /// runtime context each loop turn. After this, handlers can call
    /// `ctx.spawn_local(...)` and `.await` native Tokio futures.
    fn install_async_tokio(self) -> Self;
}

impl BastydeAppBuilderTokioExt for BastydeAppBuilder {
    fn install_async_tokio(self) -> Self {
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("bastyde-tokio: failed to build the Tokio runtime"),
        );
        let handle = AsyncRuntimeHandle::new();
        let poll_source = handle.poll_source();
        let completions = handle.completions();
        let tick_handle = handle.clone();
        let rt = runtime.clone();
        self.app_state(handle)
            .app_state(completions)
            .app_state(TokioHandle { runtime })
            .on_loop_tick(poll_source, move || {
                // Enter the runtime context so Tokio leaf futures polled during
                // this tick register with the background reactor / timer driver
                // (and register our executor's Waker as the wake target).
                let _guard = rt.enter();
                tick_handle.tick()
            })
    }
}
