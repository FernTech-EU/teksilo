//! Validates async-std integration headlessly: an `async_std::task::sleep`
//! awaited on the main-thread executor must resolve via async-std's global
//! reactor — with NO runtime-context guard around the tick.

use std::time::{Duration, Instant};

use bastyde_async::AsyncRuntimeHandle;
use bastyde_core::Signal;

#[test]
fn executor_awaits_an_async_std_timer() {
    let rt = AsyncRuntimeHandle::new();
    let done = Signal::new(false);

    let sink = done.clone();
    rt.spawn_local(async move {
        async_std::task::sleep(Duration::from_millis(50)).await;
        sink.set(true);
    })
    .detach();

    let start = Instant::now();
    while !done.get() {
        rt.tick(); // no enter-guard — async-std's reactor is global
        if start.elapsed() > Duration::from_secs(5) {
            panic!("async_std::task::sleep never resolved on the main-thread executor");
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(done.get());
}
