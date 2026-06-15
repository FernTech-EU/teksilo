// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! # bastyde-async — optional main-thread async executor for Bastyde
//!
//! Bastyde keeps the view layer synchronous: "async is the backend's concern."
//! Most background→UI flows are best served by the reactive data path
//! (`ctx.subscribe_event(...)` + `Signal::set`). This crate is the **opt-in**
//! escape hatch for the cases that want *imperative* async — writing linear
//! `async` / `.await` inside a handler, sequencing several awaits in one place.
//!
//! ```ignore
//! use bastyde_async::{BastydeAppBuilderAsyncExt, EventContextAsyncExt, spawn_blocking};
//!
//! BastydeAppBuilder::new().install_async() /* ... */ .run();
//!
//! // inside a handler:
//! let status = self.status.clone();           // Signal<Status> (Rc clone)
//! ctx.spawn_local(async move {
//!     status.set(Status::Loading);
//!     let bytes = spawn_blocking(move || std::fs::read(path)).await;
//!     status.set(Status::from(bytes));         // resume on the UI thread → set Signal
//! })
//! .detach();
//! ```
//!
//! ## Model
//!
//! - The executor is single-threaded and `!Send`; `spawn_local` futures live on
//!   the UI thread and capture `Rc`-based `Signal`s, mutating them on resume.
//!   There is no `EventContext` after `.await` (it is borrow-transient), so UI
//!   updates flow through owned handles (Signals) — the reactive model.
//! - For a one-shot ambient op after the work finishes (`open_window`,
//!   `send_intent`, …), [`spawn_local_with`](EventContextAsyncExt::spawn_local_with)
//!   delivers the result to a callback with a *fresh* `EventContext`.
//! - [`spawn_blocking`] offloads blocking work to an OS thread and awaits the
//!   result — no async runtime required.
//!
//! The executor is driven once per event-loop turn via the async-agnostic
//! [`on_loop_tick`](bastyde_app::BastydeAppBuilder::on_loop_tick) hook; it
//! sleeps (zero idle CPU) until a task is woken, including from a
//! `spawn_blocking` worker thread.
//!
//! `bastyde-tokio` / `bastyde-async-std` build on this crate to add reactor
//! support so native-ecosystem futures (`tokio::time`, sockets, `reqwest`, …)
//! can be `.await`ed directly in UI code.

mod blocking;
mod executor;
mod ext;
mod install;

pub use blocking::{BlockingError, spawn_blocking};
pub use executor::{AsyncRuntimeHandle, TaskHandle};
pub use ext::EventContextAsyncExt;
pub use install::BastydeAppBuilderAsyncExt;
