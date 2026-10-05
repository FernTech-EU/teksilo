// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The child's output between the PTY reader thread and the UI thread: a
//! bounded queue, and the thread that fills it.
//!
//! The reader thread blocks in `read`, pushes what it got, and asks the
//! terminal's [`RepaintTrigger`] for a pull; the UI thread takes from the
//! queue in the terminal's pull hook, in the layout pass of the next
//! frame, whether or not the terminal is shown, as much as it can parse in
//! one pull's budget. The queue is bounded: it never holds
//! more than [`READER_QUEUE_CAP`], since the reader reads at most the room
//! left, and with none left it stops reading until the UI thread has taken
//! some, the PTY's kernel buffer fills, and the child waits on its write.
//! That is terminal flow control for a UI thread that cannot keep up; a
//! terminal nobody is looking at still takes its output in, so it does not
//! make its child wait.

use std::collections::VecDeque;
use std::io::Read;

use teksilo_canvas::sync::{Arc, Condvar, Mutex};
use teksilo_core::RepaintTrigger;

/// The most bytes the queue holds.
pub(crate) const READER_QUEUE_CAP: usize = 4 << 20;

/// The bytes read and not yet taken. One lock and one condition, both leaves:
/// nothing else is locked while either is held, and the trigger is asked
/// only after the lock is released.
pub(crate) struct ReaderQueue {
    state: Mutex<QueueState>,
    /// Signalled when the UI thread takes bytes, and on shutdown.
    space: Condvar,
    cap: usize,
}

#[derive(Default)]
struct QueueState {
    bytes: VecDeque<u8>,
    eof: bool,
    /// The terminal is gone: the reader thread stops. Under the lock, so a
    /// reader about to wait cannot miss it.
    shut_down: bool,
    /// Readers waiting for space, so a test can tell one is.
    #[cfg(all(test, not(teksilo_loom)))]
    waiting: usize,
}

impl ReaderQueue {
    pub(crate) fn new() -> Self {
        Self::with_cap(READER_QUEUE_CAP)
    }

    pub(crate) fn with_cap(cap: usize) -> Self {
        Self {
            state: Mutex::new(QueueState::default()),
            space: Condvar::new(),
            cap,
        }
    }

    fn lock(&self) -> teksilo_canvas::sync::MutexGuard<'_, QueueState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Reader thread: wait while the queue is full, then return the room
    /// left, at least one byte; `None` once the terminal is gone. The reader
    /// is the only one that adds, so the room only grows until its push.
    pub(crate) fn wait_for_space(&self) -> Option<usize> {
        let mut state = self.lock();
        while !state.shut_down && state.bytes.len() >= self.cap {
            #[cfg(all(test, not(teksilo_loom)))]
            {
                state.waiting += 1;
            }
            state = self
                .space
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            #[cfg(all(test, not(teksilo_loom)))]
            {
                state.waiting -= 1;
            }
        }
        (!state.shut_down).then(|| self.cap - state.bytes.len())
    }

    /// Reader thread: queue `bytes`, at most the room
    /// [`wait_for_space`](Self::wait_for_space) returned. `false` once the
    /// terminal is gone.
    pub(crate) fn push(&self, bytes: &[u8]) -> bool {
        let mut state = self.lock();
        if state.shut_down {
            return false;
        }
        debug_assert!(state.bytes.len() + bytes.len() <= self.cap, "past the cap");
        state.bytes.extend(bytes);
        true
    }

    /// Reader thread: the child's output ended. `false` once the terminal is
    /// gone. Sticky.
    pub(crate) fn set_eof(&self) -> bool {
        let mut state = self.lock();
        state.eof = true;
        !state.shut_down
    }

    /// UI thread: take the oldest `max` bytes queued at most. Wakes a reader
    /// waiting for space.
    pub(crate) fn take_up_to(&self, max: usize) -> Taken {
        let mut state = self.lock();
        let n = max.min(state.bytes.len());
        let bytes: Vec<u8> = state.bytes.drain(..n).collect();
        let more = !state.bytes.is_empty();
        let eof = state.eof && !more;
        drop(state);
        if !bytes.is_empty() {
            self.space.notify_one();
        }
        Taken { bytes, eof, more }
    }

    /// UI thread: the terminal is gone. A reader waiting for space, or about
    /// to, stops. Idempotent.
    pub(crate) fn shut_down(&self) {
        self.lock().shut_down = true;
        self.space.notify_all();
    }

    pub(crate) fn is_shut_down(&self) -> bool {
        self.lock().shut_down
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.lock().bytes.len()
    }

    /// How many readers are waiting for space.
    #[cfg(all(test, not(teksilo_loom)))]
    pub(crate) fn waiting(&self) -> usize {
        self.lock().waiting
    }
}

/// What [`ReaderQueue::take_up_to`] took.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Taken {
    pub(crate) bytes: Vec<u8>,
    /// The output ended, and nothing of it is left to take.
    pub(crate) eof: bool,
    /// Bytes are left to take.
    pub(crate) more: bool,
}

/// Spawn the PTY reader thread: read at most the room left, queue, ask for a
/// pull; on the output's end, mark it and ask once more. Ends when the
/// reader reports the end of the output, fails, or the terminal is gone. A
/// reader blocked in `read` ends at the read's return (see
/// [`PtyReader`](crate::engine::PtyReader)).
pub(crate) fn spawn_reader_thread(
    mut reader: Box<dyn Read + Send>,
    queue: Arc<ReaderQueue>,
    trigger: RepaintTrigger,
) {
    let _ = std::thread::Builder::new()
        .name("teksilo-terminal-pty".into())
        .spawn(move || {
            let mut buf = [0u8; 8192];
            while let Some(room) = queue.wait_for_space() {
                let len = room.min(buf.len());
                match reader.read(&mut buf[..len]) {
                    Ok(0) => {
                        if queue.set_eof() {
                            trigger.request_pull();
                        }
                        break;
                    }
                    Ok(n) => {
                        if !queue.push(&buf[..n]) {
                            break;
                        }
                        trigger.request_pull();
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => {
                        if queue.set_eof() {
                            trigger.request_pull();
                        }
                        break;
                    }
                }
            }
        });
}

#[cfg(all(test, not(teksilo_loom)))]
mod tests {
    use std::time::Duration;

    use super::*;

    /// Until a reader waits on `queue`, within `timeout`.
    fn until_waiting(queue: &ReaderQueue, timeout: Duration) {
        let deadline = std::time::Instant::now() + timeout;
        while queue.waiting() == 0 {
            assert!(std::time::Instant::now() < deadline, "no reader waited");
            std::thread::yield_now();
        }
    }

    #[test]
    fn a_full_queue_holds_the_reader_until_bytes_are_taken() {
        let queue = Arc::new(ReaderQueue::with_cap(4));
        assert_eq!(queue.wait_for_space(), Some(4), "all the room");
        assert!(queue.push(&[1, 2, 3]));
        assert_eq!(queue.wait_for_space(), Some(1), "the room left");
        assert!(queue.push(&[4]));
        let (tx, rx) = std::sync::mpsc::channel();
        let remote = queue.clone();
        let waiter = std::thread::spawn(move || {
            let free = remote.wait_for_space();
            tx.send(free).unwrap();
        });
        until_waiting(&queue, Duration::from_secs(5));
        assert!(rx.try_recv().is_err(), "a full queue holds the reader");
        let taken = queue.take_up_to(usize::MAX);
        assert_eq!((taken.bytes, taken.eof), (vec![1, 2, 3, 4], false));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(Some(4)));
        waiter.join().unwrap();
    }

    #[test]
    fn shutting_down_releases_a_waiting_reader_and_refuses_bytes() {
        let queue = Arc::new(ReaderQueue::with_cap(1));
        assert!(queue.push(&[0]));
        let remote = queue.clone();
        let waiter = std::thread::spawn(move || remote.wait_for_space());
        until_waiting(&queue, Duration::from_secs(5));
        queue.shut_down();
        assert_eq!(waiter.join().unwrap(), None);
        assert!(!queue.push(&[1]));
        assert!(!queue.set_eof());
        assert!(queue.is_shut_down());
    }

    /// The reader never queues past the cap: each read is cut to the room
    /// left.
    #[test]
    fn the_reader_thread_never_queues_past_the_cap() {
        struct Endless;
        impl std::io::Read for Endless {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                buf.fill(b'y');
                Ok(buf.len())
            }
        }
        let queue = Arc::new(ReaderQueue::with_cap(10_000));
        spawn_reader_thread(Box::new(Endless), queue.clone(), RepaintTrigger::new());
        until_waiting(&queue, Duration::from_secs(5));
        assert_eq!(queue.len(), 10_000, "filled to the cap exactly");
        queue.shut_down();
    }

    #[test]
    fn the_end_of_the_output_is_sticky() {
        let queue = ReaderQueue::new();
        assert!(queue.set_eof());
        let ended = Taken {
            bytes: Vec::new(),
            eof: true,
            more: false,
        };
        assert_eq!(queue.take_up_to(usize::MAX), ended);
        assert_eq!(queue.take_up_to(usize::MAX), ended);
    }

    /// The oldest bytes first, at most what was asked for, and the end only
    /// once nothing is left before it.
    #[test]
    fn taking_in_parts_keeps_the_order_and_ends_last() {
        let queue = ReaderQueue::new();
        assert!(queue.push(&[1, 2, 3, 4, 5]));
        assert!(queue.set_eof());
        let first = queue.take_up_to(2);
        assert_eq!(
            (first.bytes, first.eof, first.more),
            (vec![1, 2], false, true)
        );
        let rest = queue.take_up_to(10);
        assert_eq!(
            (rest.bytes, rest.eof, rest.more),
            (vec![3, 4, 5], true, false)
        );
    }
}

#[cfg(all(test, teksilo_loom))]
mod loom_tests {
    use teksilo_canvas::sync::{model, thread};

    use super::*;

    /// A reader held by a one-byte cap still delivers every byte, in order,
    /// to a UI thread taking as it goes. Red without the notify in `take`:
    /// the UI thread then spins on a queue the waiting reader never fills.
    #[test]
    fn loom_reader_queue_backpressure_delivers_every_byte() {
        model(|| {
            let queue = Arc::new(ReaderQueue::with_cap(1));
            let reader = {
                let queue = queue.clone();
                thread::spawn(move || {
                    for byte in 0..2u8 {
                        assert_eq!(queue.wait_for_space(), Some(1));
                        assert!(queue.push(&[byte]));
                    }
                    assert!(queue.set_eof());
                })
            };
            let mut got = Vec::new();
            loop {
                let taken = queue.take_up_to(usize::MAX);
                got.extend(taken.bytes);
                if taken.eof {
                    break;
                }
                thread::yield_now();
            }
            reader.join().unwrap();
            assert_eq!(got, vec![0, 1]);
        });
    }

    /// Shutting down wakes a reader waiting for space. Red with the flag set
    /// outside the lock, or the notify made before the flag.
    #[test]
    fn loom_shutdown_wakes_a_reader_waiting_for_space() {
        model(|| {
            let queue = Arc::new(ReaderQueue::with_cap(1));
            assert!(queue.push(&[0]));
            let reader = {
                let queue = queue.clone();
                thread::spawn(move || (queue.wait_for_space(), queue.push(&[1])))
            };
            queue.shut_down();
            assert_eq!(reader.join().unwrap(), (None, false));
        });
    }
}
