// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `ctx.spawn_local(...)` — the extension trait that adds the spawn methods to
//! [`EventContext`], pulling the [`AsyncRuntimeHandle`] out of app-state.

use std::future::Future;

use teksilo_core::{EventContext, TeksiloWindowId};

use crate::executor::{AsyncRuntimeHandle, TaskHandle};

const NOT_INSTALLED: &str = "teksilo-async: AsyncRuntimeHandle not installed — call \
     TeksiloAppBuilder::install_async() at startup";

/// The window a completion is bound to when the context has no window: the id
/// `HeadlessApp` gives its stub window.
fn headless_window_id() -> TeksiloWindowId {
    TeksiloWindowId::new(0)
}

/// Spawn async work on the main-thread executor from inside an event handler.
///
/// Brought into scope with `use teksilo_async::EventContextAsyncExt;` (or via
/// the `teksilo` prelude when the `async` feature is on).
pub trait EventContextAsyncExt {
    /// Spawn a `!Send` future. It runs on the UI thread and may capture and
    /// mutate `Signal`s and other `Rc` handles directly on resume. Drop the
    /// returned [`TaskHandle`] to cancel, or call `.detach()` to fire-and-forget.
    fn spawn_local(&mut self, future: impl Future<Output = ()> + 'static) -> TaskHandle;

    /// Spawn a future and deliver its result to `on_complete` with a *fresh*
    /// [`EventContext`] bound to this window's tree — the supported way to run
    /// a one-shot ambient op (`open_window`, `send_intent`, …) once the work
    /// finishes. The future body itself runs handle-only.
    ///
    /// Outside a window context (a headless [`WidgetTree`](teksilo_core::WidgetTree)
    /// driven by `run_with_event_context`) the completion is bound to window
    /// `0`, the id `HeadlessApp` gives its stub window; deliver it with
    /// [`AsyncCompletionHandle::drain_into`](teksilo_core::AsyncCompletionHandle::drain_into).
    fn spawn_local_with<R: 'static>(
        &mut self,
        future: impl Future<Output = R> + 'static,
        on_complete: impl FnOnce(R, &mut EventContext) + 'static,
    ) -> TaskHandle;
}

impl EventContextAsyncExt for EventContext<'_> {
    fn spawn_local(&mut self, future: impl Future<Output = ()> + 'static) -> TaskHandle {
        let handle = self
            .app_state::<AsyncRuntimeHandle>()
            .expect(NOT_INSTALLED)
            .clone();
        if let Some(poster) = self.poster() {
            handle.set_poster(poster.clone());
        }
        handle.spawn_local(future)
    }

    fn spawn_local_with<R: 'static>(
        &mut self,
        future: impl Future<Output = R> + 'static,
        on_complete: impl FnOnce(R, &mut EventContext) + 'static,
    ) -> TaskHandle {
        let handle = self
            .app_state::<AsyncRuntimeHandle>()
            .expect(NOT_INSTALLED)
            .clone();
        if let Some(poster) = self.poster() {
            handle.set_poster(poster.clone());
        }
        let window_id: TeksiloWindowId = self
            .window()
            .map(|w| w.id())
            .unwrap_or_else(headless_window_id);
        handle.spawn_local_with(window_id, future, on_complete)
    }
}
