// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Validates the cooperative Tokio integration headlessly (no winit loop): a
//! `tokio::time::sleep` awaited on the main-thread executor must resolve,
//! driven by the background runtime's timer driver, when each tick runs inside
//! the runtime context.

use std::time::{Duration, Instant};

use bastyde_async::AsyncRuntimeHandle;
use bastyde_core::Signal;

#[test]
fn executor_awaits_a_tokio_timer() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let rt = AsyncRuntimeHandle::new();
    let done = Signal::new(false);

    let sink = done.clone();
    rt.spawn_local(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        sink.set(true);
    })
    .detach();

    let start = Instant::now();
    while !done.get() {
        {
            // The enter guard is what install_async_tokio wraps each tick in.
            let _guard = runtime.enter();
            rt.tick();
        }
        if start.elapsed() > Duration::from_secs(5) {
            panic!("tokio::time::sleep never resolved on the main-thread executor");
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(done.get());
}
