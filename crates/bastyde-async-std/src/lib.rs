//! # bastyde-async-std — async-std reactor for Bastyde's async executor
//!
//! Thin adapter over [`bastyde-async`](bastyde_async). Unlike Tokio, async-std
//! runs a **global** reactor that starts lazily on first use, so no per-tick
//! runtime-context guard is needed: `install_async_async_std()` is exactly
//! [`install_async`](bastyde_async::BastydeAppBuilderAsyncExt::install_async)
//! plus the async-std dependency in the tree. Native async-std futures
//! (`async_std::task::sleep`, async-std sockets, …) can be `.await`ed directly
//! inside `ctx.spawn_local(...)` bodies — when a leaf future is ready, the
//! global reactor wakes the executor's `Waker` and the loop ticks again.
//!
//! ```ignore
//! use bastyde_async_std::BastydeAppBuilderAsyncStdExt;
//! BastydeAppBuilder::new().install_async_async_std() /* ... */ .run();
//!
//! ctx.spawn_local(async move {
//!     async_std::task::sleep(std::time::Duration::from_secs(1)).await;
//!     status.set("done".into());
//! })
//! .detach();
//! ```

use bastyde_app::BastydeAppBuilder;
use bastyde_async::BastydeAppBuilderAsyncExt;

// Re-export the spawn surface so `use bastyde_async_std::*;` is enough for
// crate users that don't go through the `bastyde` umbrella prelude.
pub use bastyde_async::{EventContextAsyncExt, TaskHandle, spawn_blocking};

/// Adds [`install_async_async_std`](BastydeAppBuilderAsyncStdExt::install_async_async_std)
/// to the app builder.
pub trait BastydeAppBuilderAsyncStdExt {
    /// Install the main-thread executor for use with async-std futures. Since
    /// async-std's reactor is global and auto-starting, this is `install_async`
    /// — the value of this crate is pulling async-std into the dependency tree
    /// and providing a discoverable, parallel entry point to `bastyde-tokio`.
    fn install_async_async_std(self) -> Self;
}

impl BastydeAppBuilderAsyncStdExt for BastydeAppBuilder {
    fn install_async_async_std(self) -> Self {
        self.install_async()
    }
}
