//! [`spawn_blocking`] — run a blocking closure on a throwaway OS thread and
//! await its result on the main-thread executor, with **zero** async runtime.

use std::any::Any;
use std::future::Future;

/// Why a [`spawn_blocking`] future resolved to an error instead of a value.
#[derive(Debug, thiserror::Error)]
pub enum BlockingError {
    /// The closure panicked on the worker thread. The string is the panic
    /// message when one could be extracted from the payload.
    #[error("spawn_blocking closure panicked: {0}")]
    Panicked(String),
    /// The worker thread ended without sending a result. Should not happen in
    /// practice (the panic path is caught above); kept so the future always
    /// resolves rather than hanging.
    #[error("spawn_blocking worker ended without sending a result")]
    WorkerVanished,
}

/// Run `f` on a dedicated OS thread and resolve to its return value. Await the
/// returned future inside a
/// [`spawn_local`](crate::AsyncRuntimeHandle::spawn_local) body to keep
/// heavy/blocking work off the UI thread:
///
/// ```ignore
/// ctx.spawn_local(async move {
///     match bastyde_async::spawn_blocking(move || std::fs::read(path)).await {
///         Ok(bytes) => loaded.set(bytes.ok()),         // back on the UI thread
///         Err(e) => status.set(format!("load failed: {e}")),
///     }
/// })
/// .detach();
/// ```
///
/// Needs no async runtime: the worker is a plain `std::thread`, and the result
/// crosses back through a one-shot channel whose waker nudges the executor.
/// This is the runtime-free path; `bastyde-tokio` / `bastyde-async-std` add the
/// ability to `.await` native-ecosystem futures (timers, sockets) directly.
///
/// A panic in `f` is caught on the worker thread and surfaced as
/// [`BlockingError::Panicked`] — it does **not** unwind through the UI thread.
pub fn spawn_blocking<T, F>(f: F) -> impl Future<Output = Result<T, BlockingError>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = async_channel::bounded::<Result<T, BlockingError>>(1);
    std::thread::Builder::new()
        .name("bastyde-async-blocking".to_string())
        .spawn(move || {
            // Catch a panic in `f` so a misbehaving closure reports an error
            // instead of unwinding through the UI thread's executor tick.
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
                .map_err(|payload| BlockingError::Panicked(panic_message(payload)));
            // Capacity 1, single send — `try_send` only fails if the receiver
            // was dropped (the spawning task was cancelled); the value is then
            // simply discarded.
            let _ = tx.try_send(outcome);
        })
        .expect("bastyde-async: failed to spawn blocking worker thread");
    async move {
        rx.recv()
            .await
            .unwrap_or(Err(BlockingError::WorkerVanished))
    }
}

/// Best-effort extraction of a human-readable message from a panic payload.
fn panic_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}
