// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! loom models of the live image's protocols, on the real source, writer and
//! consumer. Under the cfg the source lock is a loom atomic with
//! parking_lot's orderings (see `lock.rs`), so a failed `try_lock` can read
//! a stale "locked", as it can on hardware. Each model names a mutation that
//! turns it red.

use std::sync::Arc as StdArc;

use super::internal::ReadAttempt;
use super::*;
use crate::sync::{AtomicUsize, Ordering, model, thread};
use crate::wake::{RedrawWaker, WakeKind};

/// Counts wakes by kind with loom atomics, so loom sees them.
struct LoomWaker {
    draw: AtomicUsize,
    layout: AtomicUsize,
}

impl RedrawWaker for LoomWaker {
    fn wake(&self, kind: WakeKind) {
        match kind {
            WakeKind::Draw => &self.draw,
            _ => &self.layout,
        }
        .fetch_add(1, Ordering::Relaxed);
    }
}

fn waker() -> StdArc<LoomWaker> {
    StdArc::new(LoomWaker {
        draw: AtomicUsize::new(0),
        layout: AtomicUsize::new(0),
    })
}

/// Every wake, whatever its kind.
fn count(waker: &LoomWaker) -> usize {
    waker.draw.load(Ordering::Relaxed) + waker.layout.load(Ordering::Relaxed)
}

/// The wakes that reach a window drawing nothing.
fn layout_wakes(waker: &LoomWaker) -> usize {
    waker.layout.load(Ordering::Relaxed)
}

/// A live source of one pixel, with a consumer whose pixel flag is already
/// raised: a commit that lands before the live pass takes it wakes nobody.
fn live_with_raised_flag() -> (
    LiveImageSource,
    LiveImageWriter,
    LiveImageConsumer,
    StdArc<LoomWaker>,
) {
    let source = LiveImageSource::new(LivePixelFormat::Rgba8);
    let writer = source.writer();
    writer.write_frame(1, 1, &[0; 4], 4).unwrap();
    let w = waker();
    let consumer = source.attach(Some(w.clone() as StdArc<dyn RedrawWaker>));
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[1; 4], 4)
        .unwrap();
    (source, writer, consumer, w)
}

/// A commit racing the live pass: either the pass, which takes the pixel
/// flag and then reads the generation, sees the commit, or the commit wakes
/// the window. Red with the generation stored after the unlock and `take`
/// made `Relaxed`, or with the flag raised before the generation is stored.
#[test]
fn loom_a_commit_is_uploaded_or_wakes() {
    model(|| {
        let (source, writer, consumer, w) = live_with_raised_flag();
        let before = count(&w);
        let expected = source.generation() + 1;
        // The producer hands its writer back: the last writer's drop would
        // disconnect the source and wake for that, hiding a lost pixel wake.
        let producer = thread::spawn(move || {
            writer
                .write_rect(PixelRect::new(0, 0, 1, 1), &[2; 4], 4)
                .unwrap();
            writer
        });
        let _ = consumer.take_pixels();
        let seen = source.generation();
        let writer = producer.join().unwrap();
        assert!(
            seen == expected || count(&w) > before,
            "commit {expected} neither seen (saw {seen}) nor woken for"
        );
        drop(writer);
    });
}

/// A size change racing the layout pre-pass: either the pre-pass, which
/// takes the geometry flag and then reads the meta, sees the new size, or
/// the change sends a layout wake, the kind a window that draws nothing
/// still receives. The pixel flag is clear, so the same commit's pixels
/// wake too: a draw wake does not count. Red with `notify` skipping the
/// geometry flag when the pixel flag woke.
#[test]
fn loom_a_size_change_reaches_layout_or_wakes() {
    model(|| {
        let (source, writer, consumer, w) = live_with_raised_flag();
        // Raise the geometry flag, so a change before the take wakes nobody
        // for layout, and clear the pixel flag, so the commit's pixels wake.
        writer.resize(2, 1).unwrap();
        let _ = consumer.take_pixels();
        let before = layout_wakes(&w);
        let producer = thread::spawn(move || {
            writer.resize(3, 1).unwrap();
            writer
        });
        let _ = consumer.take_geometry();
        let seen = source.meta().size;
        let writer = producer.join().unwrap();
        assert!(
            seen == Some((3, 1)) || layout_wakes(&w) > before,
            "the resize neither reached layout (saw {seen:?}) nor sent a layout wake"
        );
        drop(writer);
    });
}

/// A widget attaching while a producer changes the size: either the meta
/// the attachment records is the new one, or the change raised the new
/// consumer's flag and woke. Red with the meta read before the consumer is
/// published to the source.
#[test]
fn loom_attach_registers_then_reads() {
    model(|| {
        let source = LiveImageSource::new(LivePixelFormat::Rgba8);
        let writer = source.writer();
        writer.resize(1, 1).unwrap();
        let producer = thread::spawn(move || {
            writer.resize(2, 2).unwrap();
            writer
        });
        let w = waker();
        let consumer = source.attach(Some(w.clone() as StdArc<dyn RedrawWaker>));
        let recorded = consumer.layout_meta().size;
        let writer = producer.join().unwrap();
        assert!(
            recorded == Some((2, 2)) || layout_wakes(&w) > 0,
            "attached with {recorded:?} and never woken for layout"
        );
        drop(writer);
    });
}

/// A live pass that finds the lock held, against the holder releasing it:
/// either the pass got the lock, or the unlock woke its window, even with
/// every wake flag already raised. Red without the retry after registering,
/// or with the unlock reading the source's flag without taking it.
#[test]
fn loom_a_busy_read_is_handed_off() {
    model(|| {
        let (_source, writer, consumer, w) = live_with_raised_flag();
        let before = count(&w);
        let holder = thread::spawn(move || {
            let guard = writer.lock().unwrap();
            drop(guard);
            writer
        });
        let locked = matches!(consumer.try_read(), ReadAttempt::Locked(_));
        let writer = holder.join().unwrap();
        assert!(
            locked || count(&w) > before,
            "busy, and no unlock woke the window"
        );
        drop(writer);
    });
}

/// The last writer of a session dropping while another thread takes a new
/// writer and writes: a live writer always has its frame, and the status
/// ends `Live` with one writer. Red with the writer count kept outside the
/// state lock.
#[test]
fn loom_the_last_writers_drop_races_a_new_writer() {
    model(|| {
        let source = LiveImageSource::new(LivePixelFormat::Rgba8);
        let old = source.writer();
        old.write_frame(1, 1, &[0; 4], 4).unwrap();
        let dropper = thread::spawn(move || drop(old));
        let new = source.writer();
        new.resize(1, 1).unwrap();
        new.write_rect(PixelRect::new(0, 0, 1, 1), &[1; 4], 4)
            .expect("a live writer has a frame after its resize");
        dropper.join().unwrap();
        assert_eq!(source.status(), LiveImageStatus::Live);
        assert_eq!(source.stats().writers, 1);
        drop(new);
        assert_eq!(source.status(), LiveImageStatus::Disconnected);
    });
}
