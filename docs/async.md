<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Async Runtime Reference

**Scope:** the optional, opt-in `bastyde-async` crate (plus the `bastyde-tokio`
/ `bastyde-async-std` reactor adapters) — a **main-thread async executor** for
*imperative* async inside UI handlers.

Mental model in one line:

```text
BastydeAppBuilder::install_async()  →  ctx.spawn_local(async move { … })  →  Signal::set(result)
```

Bastyde keeps the view layer synchronous: **async is the backend's concern.**
This crate is the escape hatch for the cases where a *handler* wants to write
linear `async` / `.await` — sequencing or branching several awaits in one place
— instead of restructuring into callbacks. It is **off by default**; nothing in
`bastyde-core` or `bastyde-app` gains an async dependency unless you opt in.

## When to use it (and when not to)

| You want… | Use |
| --- | --- |
| Background work → push a result into the reactive UI | The **data path**: `ctx.subscribe_event(...)` + `Signal::set` (no executor). See [architecture.md](architecture.md) §9.4. |
| A handler that does `let a = f().await; let b = g(a).await; sig.set(b);` | `ctx.spawn_local(...)` (this crate). |
| Offload one blocking call and await its result | `spawn_blocking(...)` (this crate). |
| `.await` a native `tokio` / `async-std` future (timer, socket, `reqwest`) | `bastyde-tokio` / `bastyde-async-std`. |

For the common "kick off work, update the UI when it lands" case the reactive
data path is simpler and needs no executor — reach for `spawn_local` only when
the *imperative* shape genuinely reads better. (Bastyde apps backed by a data
layer such as Qleany generally keep async in that layer entirely.)

## The three crates

| Crate | Adds | Depends on |
| --- | --- | --- |
| `bastyde-async` | the executor, `spawn_local` / `spawn_local_with`, `spawn_blocking`, `install_async()` | `bastyde-app`, `bastyde-core` (+ `async-channel`, `thiserror`) |
| `bastyde-tokio` | `install_async_tokio()` + `TokioHandle`; awaits native Tokio futures | `bastyde-async` + `tokio` |
| `bastyde-async-std` | `install_async_async_std()`; awaits native async-std futures | `bastyde-async` + `async-std` |

`bastyde-async` alone is **runtime-free**: `spawn_blocking` offloads to a plain
`std::thread`, so you can run blocking work and await its result with no async
runtime at all. The adapter crates only add the ability to `.await` native
ecosystem futures directly.

Through the umbrella `bastyde` crate these are the `async`, `tokio`, and
`async-std` features (the latter two imply `async`). The spawn surface is in
the prelude when enabled.

## Quick start

```rust
use bastyde::prelude::*;            // brings the spawn extension traits when `async` is on

fn main() {
    BastydeAppBuilder::new()
        .theme(intui::light())
        .install_async()            // ← register the executor
        .initial_window(/* … */)
        .run();
}

// inside an event handler (`&mut EventContext`):
let status = self.status.clone();   // Signal<Status> (Rc clone)
ctx.spawn_local(async move {
    status.set(Status::Loading);
    // spawn_blocking returns Result<T, BlockingError> (Err only if it panics)
    let report = spawn_blocking(move || expensive_report(&input)).await;
    status.set(match report {                                       // resume on the UI thread
        Ok(r) => Status::Ready(r),
        Err(e) => Status::Failed(e.to_string()),
    });
})
.detach();                          // fire-and-forget; drop the handle instead to cancel
```

Demo: `cargo run -p async-demo`.

## The owned-handles model

A `spawn_local` future is single-threaded (`!Send`) and runs on the UI thread.
It captures `Rc`-based `Signal` handles and mutates them **on resume** — that is
how an async result reaches the UI. There is **no `EventContext` after `.await`**
(it is borrow-transient — it exists only during a synchronous event dispatch),
so UI updates flow through owned handles, exactly matching the reactive model.
`spawn_local` is fire-and-forget — its future's output is `()`; surface results
by setting a `Signal`, or use `spawn_local_with` (below) for a one-shot callback
that runs with a context.

This is deliberately the same shape as Slint's `spawn_local`: capture
component/state handles, set them on resume.

### `spawn_local_with` — a fresh context for one-shot ambient ops

When the *result* needs an ambient op that requires an `EventContext`
(`open_window`, `send_intent`, `set_theme`, …), use `spawn_local_with`. The
future body runs handle-only; the result is delivered to a callback with a
**fresh `EventContext`** bound to the originating window's tree:

```rust
ctx.spawn_local_with(
    async move { fetch_report(url).await },     // body: handle-only
    move |report, ctx: &mut EventContext| {      // completion: real ctx, on the origin window
        ctx.open_window(WindowConfig::new().title("Report").root(/* report */));
    },
)
.detach();                                       // keep it alive — dropping the handle cancels
```

For a multi-step sequence of ambient ops, chain: the completion callback can
itself spawn the next future. There is intentionally **no** re-entrant
"current context" available mid-future — that would couple the executor to
window internals and reopen the `RefCell` double-borrow class. (It could be
added later as a separate, additive API if a real need appears.)

## `spawn_blocking`

```rust
let result = bastyde_async::spawn_blocking(move || expensive_sync_call()).await;
// result: Result<T, BlockingError>
```

Runs the closure on a dedicated `std::thread` and resolves to
`Result<T, BlockingError>` through a one-shot channel. Needs no async runtime —
the channel's waker nudges the executor when the worker finishes. The closure
and its result must be `Send`; the awaiting task stays on the UI thread. A panic
in the closure is **caught** on the worker and surfaced as
`BlockingError::Panicked` — it does not unwind through the UI thread.

## Threading & the loop hook (zero idle cost)

The executor is driven once per event-loop turn by an **async-agnostic** hook
in `bastyde-app`:

```rust
BastydeAppBuilder::on_loop_tick(poll_source: Rc<Cell<bool>>, tick: impl FnMut() -> bool)
```

`bastyde-app` only ever sees `FnMut` + `Rc<Cell<bool>>` — it has no async
dependency. Each turn (`about_to_wait`) the hook polls the executor; a `true`
return triggers a repaint of the open windows (a task may have mutated a
`Signal`). While idle the loop sleeps in `ControlFlow::Wait` (zero CPU) until a
task is woken.

The wake path is the crux of the cross-thread story. Every task's leaf futures
are polled with **one shared `Waker`** (`Arc<ExecWaker>`, `Send + Sync`). On
wake — possibly from a `spawn_blocking` worker thread or a runtime's reactor
thread — it sets an atomic flag and nudges the winit event loop through the
(`Send + Sync`) `AppEventPoster`. It never touches the `!Send` task queue; the
main thread re-polls live tasks on the next tick. Tasks are dropped on
completion; dropping a `TaskHandle` cancels (the future is dropped on the next
tick), and `.detach()` lets it run independently.

## Reactor notes: tokio vs async-std

- **`bastyde-tokio`** owns a multi-thread `tokio::runtime::Runtime` (reactor +
  timer driver on background threads). `install_async_tokio()` wraps each tick
  in `runtime.enter()`, so a Tokio leaf future polled on the UI thread
  registers with that background driver *and* registers the executor's `Waker`
  as its wake target. When the timer/socket is ready, the background driver
  wakes the executor and the loop ticks again. `TokioHandle` (in app-state)
  exposes `.spawn()` for `Send` tasks and `.handle()`.
- **`bastyde-async-std`** needs no per-tick guard — async-std's reactor is
  global and auto-starting, so `install_async_async_std()` is just
  `install_async()` plus the async-std dependency.

Both are validated headlessly (a real `sleep` awaited on the executor resolves)
in each crate's `tests/`.

## Relationship to the subscription data path

The reactive data path (`EventSource` / `ctx.subscribe_event`) and this executor
are complementary, not competing:

- A background **publisher** (a Qleany `LongOperation`, a file watcher, a
  message bus) → `subscribe_event` → `Signal::set`. No executor; the result is
  pushed in. Best for "data arrives, UI reacts."
- An **imperative flow** that sequences/branches awaits in one handler →
  `spawn_local`. Best when the callback shape would fragment the logic.

Both deliver their effects on the UI thread and both update the UI through
`Signal`s.

## Limitations

- `spawn_local` futures cannot hold an `EventContext` across `.await`; ambient
  ops post-await go through `spawn_local_with`'s completion callback or a
  `Signal` an `Action` watches.
- A `spawn_blocking` closure panic is caught and returned as
  `BlockingError::Panicked`. A panic in a `spawn_local` *body* (your own async
  code) still propagates on the UI thread — keep those panic-free.
- The adapters bring their runtime as a normal dependency; enabling both `tokio`
  and `async-std` in one binary pulls both runtimes (rarely desirable).
- Task progress repaints all open windows (not just the one whose `Signal`
  changed), matching the `subscribe_event` data path. Negligible for single-
  window apps; a per-window targeted repaint would need framework-level dirty
  tracking.

## Code reference

| Concern | File |
| --- | --- |
| Executor, `AsyncRuntimeHandle`, `TaskHandle`, cross-thread waker | [crates/bastyde-async/src/executor.rs](../crates/bastyde-async/src/executor.rs) |
| `spawn_blocking` | [crates/bastyde-async/src/blocking.rs](../crates/bastyde-async/src/blocking.rs) |
| `EventContextAsyncExt` (`spawn_local` / `spawn_local_with`) | [crates/bastyde-async/src/ext.rs](../crates/bastyde-async/src/ext.rs) |
| `install_async()` | [crates/bastyde-async/src/install.rs](../crates/bastyde-async/src/install.rs) |
| Completion router (registry + `Send` payload) | [crates/bastyde-core/src/async_completion.rs](../crates/bastyde-core/src/async_completion.rs) |
| Neutral loop hook (`on_loop_tick`, poll source) | [crates/bastyde-app/src/app.rs](../crates/bastyde-app/src/app.rs) |
| Completion routing + window-close purge | [crates/bastyde-app/src/app.rs](../crates/bastyde-app/src/app.rs), [window_manager.rs](../crates/bastyde-app/src/window_manager.rs) |
| Tokio adapter | [crates/bastyde-tokio/src/lib.rs](../crates/bastyde-tokio/src/lib.rs) |
| async-std adapter | [crates/bastyde-async-std/src/lib.rs](../crates/bastyde-async-std/src/lib.rs) |
| Demo | [examples/async_demo/src/main.rs](../examples/async_demo/src/main.rs) |
