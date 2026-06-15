// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `install_async()` — the builder hook that wires the executor into the app.

use bastyde_app::BastydeAppBuilder;

use crate::executor::AsyncRuntimeHandle;

/// Adds [`install_async`](BastydeAppBuilderAsyncExt::install_async) to the app
/// builder. Brought into scope with `use bastyde_async::BastydeAppBuilderAsyncExt;`
/// (or via the `bastyde` prelude when the `async` feature is on).
pub trait BastydeAppBuilderAsyncExt {
    /// Install the main-thread async runtime: register the
    /// [`AsyncRuntimeHandle`] and its completion registry in app-state, and
    /// wire the executor poll into the event loop via
    /// [`on_loop_tick`](BastydeAppBuilder::on_loop_tick). After this, handlers
    /// can call `ctx.spawn_local(...)` /
    /// [`spawn_blocking`](crate::spawn_blocking).
    fn install_async(self) -> Self;
}

impl BastydeAppBuilderAsyncExt for BastydeAppBuilder {
    fn install_async(self) -> Self {
        let handle = AsyncRuntimeHandle::new();
        let poll_source = handle.poll_source();
        // The completion registry is a bastyde-core type, so bastyde-app can
        // fetch it from app-state and route `spawn_local_with` completions
        // without depending on bastyde-async (which would be a cycle).
        let completions = handle.completions();
        let tick_handle = handle.clone();
        let waker_handle = handle.clone();
        self.app_state(handle)
            .app_state(completions)
            .on_loop_tick(poll_source, move || tick_handle.tick())
            // Wire the cross-thread waker at startup (the AppEventProxy is itself
            // an AppEventPoster), so a spawn wakes the loop even when the app
            // uses the handle directly rather than via `ctx.spawn_local`. The
            // ext trait also sets it lazily; `set_poster` is idempotent.
            .on_ready(move |proxy| waker_handle.set_poster(std::sync::Arc::new(proxy)))
    }
}
