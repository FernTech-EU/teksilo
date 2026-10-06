// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The source protocol, headless: errors, transactions, the status machine,
//! sessions, the planner, the wake flags and the busy-lock handoff (spec
//! A.1-A.16, A.18), and two stress runs (spec 11.4).

use std::sync::Arc;
use std::time::Duration;

use super::consumer::RenderRecord;
use super::internal::{LiveMeta, ReadAttempt, UploadPlan};
use super::source::{test_hooks, validate_size};
use super::*;
use crate::wake::{CountingWaker, RedrawWaker, WakeKind};

fn source() -> LiveImageSource {
    LiveImageSource::builder(LivePixelFormat::Rgba8)
        .label("test")
        .build()
}

/// A `w × h` frame whose pixel `(x, y)` is `[x, y, seed, 255]`, wrapped to
/// bytes.
fn frame(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            px.extend_from_slice(&[x as u8, y as u8, seed, 255]);
        }
    }
    px
}

fn attach(source: &LiveImageSource) -> (Arc<CountingWaker>, LiveImageConsumer) {
    let waker = Arc::new(CountingWaker::new());
    let consumer = source.attach(Some(waker.clone() as Arc<dyn RedrawWaker>));
    (waker, consumer)
}

/// What a consumer at `cursor` would upload now.
fn plan(consumer: &LiveImageConsumer, cursor: Option<u64>) -> UploadPlan {
    match consumer.try_read() {
        ReadAttempt::Locked(read) => read.plan(cursor),
        ReadAttempt::Busy => panic!("the source is locked"),
    }
}

fn pixels(consumer: &LiveImageConsumer) -> (Option<(u32, u32)>, Vec<u8>, u64) {
    match consumer.try_read() {
        ReadAttempt::Locked(read) => (read.size(), read.pixels().to_vec(), read.generation()),
        ReadAttempt::Busy => panic!("the source is locked"),
    }
}

fn rects(plan: UploadPlan) -> Vec<PixelRect> {
    match plan {
        UploadPlan::Rects(r) => r.as_slice().to_vec(),
        other => panic!("expected rects, got {other:?}"),
    }
}

// ── A.1: every error, and a refused call changes nothing ──

#[test]
fn every_error_is_returned_and_changes_nothing() {
    let source = source();
    let writer = source.writer();
    let (_, consumer) = attach(&source);
    assert_eq!(
        writer.write_rect(PixelRect::new(0, 0, 1, 1), &[0; 4], 4),
        Err(LiveImageError::NoFrame)
    );
    assert_eq!(
        writer.resize(0, 5),
        Err(LiveImageError::ZeroSize {
            width: 0,
            height: 5
        })
    );
    assert_eq!(
        writer.resize(16_385, 1),
        Err(LiveImageError::TooLarge {
            width: 16_385,
            height: 1
        })
    );
    writer.write_frame(4, 3, &frame(4, 3, 1), 16).unwrap();
    let before = pixels(&consumer);
    let generation = source.generation();
    let refusals = [
        writer.write_rect(PixelRect::new(3, 0, 2, 1), &[0; 8], 8),
        writer.write_rect(PixelRect::new(0, 0, 2, 2), &[0; 16], 4),
        writer.write_rect(PixelRect::new(0, 0, 2, 2), &[0; 11], 8),
        writer.write_frame(4, 3, &[0; 47], 16),
        writer.write_frame(0, 3, &[], 0),
        writer.resize(3, 16_385),
    ];
    assert_eq!(
        refusals.to_vec(),
        vec![
            Err(LiveImageError::OutOfBounds {
                rect: PixelRect::new(3, 0, 2, 1),
                width: 4,
                height: 3
            }),
            Err(LiveImageError::StrideTooSmall { stride: 4, min: 8 }),
            Err(LiveImageError::BufferTooSmall {
                len: 11,
                needed: 16
            }),
            Err(LiveImageError::BufferTooSmall {
                len: 47,
                needed: 48
            }),
            Err(LiveImageError::ZeroSize {
                width: 0,
                height: 3
            }),
            Err(LiveImageError::TooLarge {
                width: 3,
                height: 16_385
            }),
        ]
    );
    test_hooks::fail_next_reserve();
    assert_eq!(
        writer.resize(8, 8),
        Err(LiveImageError::OutOfMemory { bytes: 256 })
    );
    let rejected = writer.swap_frame(4, 3, 8, vec![7; 48]).unwrap_err();
    assert_eq!(
        rejected.error,
        LiveImageError::StrideTooSmall { stride: 8, min: 16 }
    );
    assert_eq!(source.generation(), generation, "no refusal published");
    assert_eq!(pixels(&consumer), before, "no refusal touched the pixels");
    assert_eq!(
        plan(&consumer, Some(generation)),
        UploadPlan::Nothing,
        "nor marked damage"
    );
    assert_eq!(
        source.stats().abandoned,
        0,
        "a refusal is not an abandoned transaction"
    );
    assert!(
        validate_size(
            LiveImageSource::MAX_DIMENSION,
            LiveImageSource::MAX_DIMENSION
        )
        .is_ok(),
        "the largest frame is accepted without allocating it"
    );
    assert_eq!(
        source.set_size_hint(0, 1),
        Err(LiveImageError::ZeroSize {
            width: 0,
            height: 1
        })
    );
}

#[test]
fn a_contended_try_lock_would_block_and_a_revoked_writer_is_refused() {
    let source = source();
    let writer = source.writer();
    writer.resize(2, 2).unwrap();
    let (held, release) = (
        Arc::new(std::sync::Barrier::new(2)),
        Arc::new(std::sync::Barrier::new(2)),
    );
    let holder = {
        let (writer, held, release) = (writer.clone(), held.clone(), release.clone());
        std::thread::spawn(move || {
            let guard = writer.lock().unwrap();
            held.wait();
            release.wait();
            drop(guard);
        })
    };
    held.wait();
    assert!(matches!(writer.try_lock(), Err(LiveImageError::WouldBlock)));
    release.wait();
    holder.join().unwrap();
    assert!(writer.try_lock().is_ok());

    let fresh = source.writer_exclusive();
    assert!(matches!(writer.lock(), Err(LiveImageError::Revoked)));
    assert!(matches!(writer.try_lock(), Err(LiveImageError::Revoked)));
    assert_eq!(writer.resize(9, 9), Err(LiveImageError::Revoked));
    assert_eq!(
        writer.write_frame(2, 2, &[0; 16], 8),
        Err(LiveImageError::Revoked)
    );
    assert_eq!(writer.clear(), Err(LiveImageError::Revoked));
    assert_eq!(
        writer.swap_frame(2, 2, 8, vec![0; 16]).unwrap_err().error,
        LiveImageError::Revoked
    );
    drop(writer);
    assert_eq!(
        source.status(),
        LiveImageStatus::Waiting,
        "the old drop changed nothing"
    );
    assert_eq!(source.stats().writers, 1);
    drop(fresh);
    assert_eq!(source.status(), LiveImageStatus::Disconnected);
}

// ── A.2, A.3: one commit is one generation; an abandoned transaction ──

#[test]
fn one_commit_is_one_generation_and_an_empty_one_is_none() {
    let source = source();
    let writer = source.writer();
    writer.resize(8, 8).unwrap();
    let (waker, consumer) = attach(&source);
    let start = source.generation();
    let mut guard = writer.lock().unwrap();
    for i in 0..5 {
        guard
            .write_rect(PixelRect::new(i, i, 1, 1), &[1, 2, 3, 4], 4)
            .unwrap();
    }
    assert_eq!(guard.commit(), start + 1);
    assert_eq!(source.generation(), start + 1);
    assert_eq!(waker.count(), 1);
    consumer.take_pixels();
    let empty = writer.lock().unwrap().commit();
    assert_eq!(empty, start + 1, "nothing marked: nothing published");
    assert_eq!(waker.count(), 1, "an empty commit wakes nobody");
    assert_eq!(source.stats().commits, 2, "the resize and the writes");
}

#[test]
fn an_abandoned_transaction_publishes_nothing_and_keeps_its_damage() {
    let source = source();
    let writer = source.writer();
    writer.resize(16, 16).unwrap();
    let (waker, consumer) = attach(&source);
    let start = source.generation();
    {
        let mut guard = writer.lock().unwrap();
        guard
            .write_rect(PixelRect::new(2, 3, 2, 2), &[9; 16], 8)
            .unwrap();
    }
    assert_eq!(source.generation(), start);
    assert_eq!(waker.count(), 0, "an abandoned transaction wakes nobody");
    assert_eq!(source.stats().abandoned, 1);
    let mut guard = writer.lock().unwrap();
    guard
        .write_rect(PixelRect::new(10, 10, 1, 1), &[1; 4], 4)
        .unwrap();
    assert_eq!(guard.commit(), start + 1);
    assert_eq!(
        rects(plan(&consumer, Some(start))),
        vec![PixelRect::new(2, 3, 2, 2), PixelRect::new(10, 10, 1, 1)],
        "the abandoned damage reaches the next commit"
    );
}

// ── A.4: a panic inside a transaction ──

#[test]
fn a_panic_inside_a_transaction_publishes_nothing_and_forces_full() {
    let source = source();
    let writer = source.writer();
    writer.resize(16, 16).unwrap();
    let (waker, consumer) = attach(&source);
    let start = source.generation();
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut guard = writer.lock().unwrap();
        guard
            .write_rect(PixelRect::new(0, 0, 1, 1), &[1; 4], 4)
            .unwrap();
        panic!("decoder failed");
    }));
    assert!(panicked.is_err());
    assert_eq!(source.generation(), start);
    assert_eq!(waker.count(), 0);
    assert_eq!(source.stats().abandoned, 1);
    // Still readable and writable: parking_lot does not poison.
    let mut guard = writer.lock().unwrap();
    guard
        .write_rect(PixelRect::new(5, 5, 1, 1), &[2; 4], 4)
        .unwrap();
    guard.commit();
    assert_eq!(plan(&consumer, Some(start)), UploadPlan::Full);
}

// ── A.5, A.6: the copies ──

#[test]
fn copy_within_matches_a_reference_memmove_in_every_direction() {
    let (w, h) = (9u32, 7u32);
    let moves = [
        (PixelRect::new(1, 2, 5, 3), 2, 3), // down and right, overlapping
        (PixelRect::new(2, 3, 5, 3), 1, 2), // up and left, overlapping
        (PixelRect::new(0, 1, 4, 4), 0, 3), // straight down
        (PixelRect::new(0, 3, 4, 4), 0, 0), // straight up
        (PixelRect::new(1, 0, 6, 7), 3, 0), // right in each row
        (PixelRect::new(3, 0, 6, 7), 0, 0), // left in each row
        (PixelRect::new(0, 0, 0, 3), 5, 4), // empty
    ];
    for (from, to_x, to_y) in moves {
        let source = source();
        let writer = source.writer();
        let start = frame(w, h, 7);
        writer.write_frame(w, h, &start, (w * 4) as usize).unwrap();
        let mut guard = writer.lock().unwrap();
        guard.copy_within(from, to_x, to_y).unwrap();
        guard.commit();
        // Reference: copy the source region out first, then write it.
        let mut expected = start.clone();
        let row = (from.width * 4) as usize;
        let rows: Vec<Vec<u8>> = (0..from.height)
            .map(|y| {
                let at = (((from.y + y) * w + from.x) * 4) as usize;
                start[at..at + row].to_vec()
            })
            .collect();
        for (y, bytes) in rows.iter().enumerate() {
            let at = (((to_y + y as u32) * w + to_x) * 4) as usize;
            expected[at..at + row].copy_from_slice(bytes);
        }
        let (_, consumer) = attach(&source);
        assert_eq!(
            pixels(&consumer).1,
            expected,
            "{from:?} to ({to_x}, {to_y})"
        );
    }
}

#[test]
fn write_rect_honours_a_wide_source_stride_and_rows_mut_yields_the_rect() {
    let source = source();
    let writer = source.writer();
    writer.resize(6, 4).unwrap();
    // A 2 × 3 rect in a buffer with 5 pixels per row.
    let mut src = vec![0u8; 5 * 4 * 3];
    for y in 0..3 {
        for x in 0..2 {
            let at = (y * 5 + x) * 4;
            src[at..at + 4].copy_from_slice(&[10 + x as u8, 20 + y as u8, 0, 255]);
        }
    }
    writer
        .write_rect(PixelRect::new(3, 1, 2, 3), &src, 5 * 4)
        .unwrap();
    let (_, consumer) = attach(&source);
    let (_, px, _) = pixels(&consumer);
    for y in 0..3usize {
        for x in 0..2usize {
            let at = ((1 + y) * 6 + 3 + x) * 4;
            assert_eq!(&px[at..at + 2], &[10 + x as u8, 20 + y as u8], "({x}, {y})");
        }
    }
    let mut guard = writer.lock().unwrap();
    let mut rows = guard.rows_mut(PixelRect::new(1, 1, 3, 2)).unwrap();
    assert_eq!((rows.width(), rows.height()), (3, 2));
    for y in 0..rows.height() {
        let row = rows.row_mut(y);
        assert_eq!(row.len(), 12);
        row.fill(0xAB);
    }
    let generation = guard.commit();
    let (_, px, _) = pixels(&consumer);
    assert_eq!(&px[(6 + 1) * 4..(6 + 4) * 4], &[0xAB; 12][..]);
    assert_eq!(
        rects(plan(&consumer, Some(generation - 1))),
        vec![PixelRect::new(1, 1, 3, 2)]
    );
}

#[test]
fn fill_rect_writes_the_pixel_and_mark_dirty_clamps() {
    let source = source();
    let writer = source.writer();
    writer.resize(4, 4).unwrap();
    let (_, consumer) = attach(&source);
    let start = source.generation();
    let mut guard = writer.lock().unwrap();
    guard
        .fill_rect(PixelRect::new(1, 1, 2, 1), [1, 2, 3, 4])
        .unwrap();
    guard.pixels_mut()[0] = 99;
    guard.mark_dirty(PixelRect::new(3, 3, 10, 10));
    guard.mark_dirty(PixelRect::new(10, 10, 1, 1));
    guard.mark_dirty(PixelRect::new(0, 0, 0, 1));
    guard.commit();
    assert_eq!(
        rects(plan(&consumer, Some(start))),
        vec![PixelRect::new(1, 1, 2, 1), PixelRect::new(3, 3, 1, 1)]
    );
    let (_, px, _) = pixels(&consumer);
    assert_eq!(&px[(4 + 1) * 4..(4 + 3) * 4], &[1, 2, 3, 4, 1, 2, 3, 4]);
    assert_eq!(px[0], 99, "written in place, if unmarked");
}

// ── A.7, A.8, A.9: the status machine ──

#[test]
fn the_status_follows_writers_pixels_and_clear() {
    let source = LiveImageSource::builder(LivePixelFormat::Bgrx8)
        .size_hint(720, 1280)
        .build();
    assert_eq!(source.status(), LiveImageStatus::Disconnected);
    assert_eq!(source.size(), Some((720, 1280)), "the hint");
    let writer = source.writer();
    assert_eq!(source.status(), LiveImageStatus::Waiting);
    writer.resize(3, 2).unwrap();
    assert_eq!(
        source.status(),
        LiveImageStatus::Waiting,
        "a resize is no pixels"
    );
    assert_eq!(
        source.size(),
        Some((3, 2)),
        "the buffer's size wins over the hint"
    );
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[0; 4], 4)
        .unwrap();
    assert_eq!(source.status(), LiveImageStatus::Live);
    writer.resize(2, 3).unwrap();
    assert_eq!(
        source.status(),
        LiveImageStatus::Live,
        "a resize keeps Live"
    );
    writer.clear().unwrap();
    assert_eq!(source.status(), LiveImageStatus::Waiting);
    assert_eq!(
        source.size(),
        Some((2, 3)),
        "the freed size became the hint"
    );
    writer.write_frame(1, 1, &[0; 4], 4).unwrap();
    assert_eq!(source.status(), LiveImageStatus::Live);
    let (_, consumer) = attach(&source);
    drop(writer);
    assert_eq!(source.status(), LiveImageStatus::Disconnected);
    assert_eq!(pixels(&consumer).0, None, "the pixels were freed");
    assert_eq!(source.size(), Some((1, 1)));
    let writer = source.writer();
    assert_eq!(source.status(), LiveImageStatus::Waiting);
    drop(writer);
}

#[test]
fn the_last_writers_drop_frees_the_buffer() {
    let source = source();
    let writer = source.writer();
    writer.write_frame(64, 64, &frame(64, 64, 0), 256).unwrap();
    let second = writer.clone();
    drop(writer);
    assert_eq!(
        source.status(),
        LiveImageStatus::Live,
        "a clone shares the token"
    );
    let other = source.writer();
    drop(second);
    assert_eq!(
        source.status(),
        LiveImageStatus::Live,
        "another token lives"
    );
    drop(other);
    assert_eq!(source.status(), LiveImageStatus::Disconnected);
    let (_, consumer) = attach(&source);
    let ReadAttempt::Locked(read) = consumer.try_read() else {
        panic!("locked")
    };
    assert!(read.pixels().is_empty());
    assert_eq!(read.size(), None);
}

#[test]
fn a_same_size_resize_changes_nothing_and_wakes_nobody() {
    let source = source();
    let writer = source.writer();
    writer.write_frame(5, 5, &frame(5, 5, 3), 20).unwrap();
    let (waker, consumer) = attach(&source);
    let generation = source.generation();
    assert_eq!(writer.resize(5, 5), Ok(generation));
    let mut guard = writer.lock().unwrap();
    guard.resize(5, 5).unwrap();
    assert_eq!(guard.commit(), generation);
    assert_eq!(waker.count(), 0);
    assert_eq!(pixels(&consumer).1, frame(5, 5, 3), "not zero-filled");
}

// ── A.10, A.11: sessions and swap_frame ──

#[test]
fn writer_exclusive_starts_a_waiting_session_and_frees_the_pixels() {
    let source = source();
    let old = source.writer();
    old.write_frame(4, 4, &frame(4, 4, 1), 16).unwrap();
    let new = source.writer_exclusive();
    assert_eq!(source.status(), LiveImageStatus::Waiting);
    assert_eq!(source.size(), Some((4, 4)), "the old size is the hint");
    let (_, consumer) = attach(&source);
    assert_eq!(
        pixels(&consumer).0,
        None,
        "the old session's pixels are gone"
    );
    assert_eq!(old.resize(4, 4), Err(LiveImageError::Revoked));
    new.write_frame(2, 2, &frame(2, 2, 9), 8).unwrap();
    assert_eq!(source.status(), LiveImageStatus::Live);
}

#[test]
fn swap_frame_hands_the_previous_buffer_back() {
    let source = source();
    let writer = source.writer();
    let first = frame(4, 2, 1);
    let first_ptr = first.as_ptr();
    let previous = writer.swap_frame(4, 2, 16, first).unwrap();
    assert!(previous.is_empty(), "nothing before the first frame");
    let second = frame(4, 2, 2);
    let second_ptr = second.as_ptr();
    let previous = writer.swap_frame(4, 2, 16, second).unwrap();
    assert_eq!(
        previous.as_ptr(),
        first_ptr,
        "the buffer itself, not a copy"
    );
    let (_, consumer) = attach(&source);
    let ReadAttempt::Locked(read) = consumer.try_read() else {
        panic!("locked")
    };
    assert_eq!(
        read.pixels().as_ptr(),
        second_ptr,
        "installed without a copy"
    );
    drop(read);
    let short = vec![0u8; 31];
    let short_ptr = short.as_ptr();
    let rejected = writer.swap_frame(4, 2, 16, short).unwrap_err();
    assert_eq!(rejected.pixels.as_ptr(), short_ptr, "refused untouched");
    assert_eq!(
        rejected.error,
        LiveImageError::BufferTooSmall {
            len: 31,
            needed: 32
        }
    );
    // A wide stride is kept, and a frame installed at it uploads in full.
    writer.swap_frame(4, 2, 24, vec![5; 40]).unwrap();
    let ReadAttempt::Locked(read) = consumer.try_read() else {
        panic!("locked")
    };
    assert_eq!(read.stride(), 24);
    assert_eq!(read.plan(Some(read.generation() - 1)), UploadPlan::Full);
}

// ── A.12, A.13: threading ──

#[test]
fn the_source_and_writer_are_send_and_sync_and_debug_never_waits() {
    fn send_sync<T: Send + Sync + Clone>() {}
    send_sync::<LiveImageSource>();
    send_sync::<LiveImageWriter>();
    send_sync::<LiveImageConsumer>();
    let source = source();
    let writer = source.writer();
    writer.write_frame(2, 2, &[0; 16], 8).unwrap();
    let (held, release) = (
        Arc::new(std::sync::Barrier::new(2)),
        Arc::new(std::sync::Barrier::new(2)),
    );
    let holder = {
        let (writer, held, release) = (writer.clone(), held.clone(), release.clone());
        std::thread::spawn(move || {
            let guard = writer.lock().unwrap();
            held.wait();
            release.wait();
            drop(guard);
        })
    };
    held.wait();
    let shown = format!("{source:?} {writer:?}");
    assert!(shown.contains("\"test\" Rgba8 2x2 Live gen 1"), "{shown}");
    let _ = source.stats();
    release.wait();
    holder.join().unwrap();
}

#[test]
#[should_panic(expected = "live image #")]
fn locking_twice_on_one_thread_panics_naming_the_source() {
    let source = source();
    let writer = source.writer();
    let other = writer.clone();
    let _guard = writer.lock().unwrap();
    let _ = other.lock();
}

/// `call` panics, naming the source and why.
fn panics_naming_the_source(name: &str, call: impl FnOnce()) {
    let message = std::panic::catch_unwind(std::panic::AssertUnwindSafe(call))
        .expect_err(name)
        .downcast::<String>()
        .map(|s| *s)
        .unwrap_or_default();
    assert!(
        message.contains("\"test\"") && message.contains("not reentrant"),
        "{name}: {message}"
    );
}

#[test]
fn a_helper_or_session_call_inside_a_transaction_panics_and_try_lock_would_block() {
    let source = source();
    let writer = source.writer();
    let guard = writer.lock().unwrap();
    assert!(matches!(writer.try_lock(), Err(LiveImageError::WouldBlock)));
    panics_naming_the_source("write_frame", || drop(writer.write_frame(1, 1, &[0; 4], 4)));
    panics_naming_the_source("writer", || drop(source.writer()));
    panics_naming_the_source("writer_exclusive", || drop(source.writer_exclusive()));
    panics_naming_the_source("set_size_hint", || drop(source.set_size_hint(2, 2)));
    panics_naming_the_source("clear", || drop(writer.clear()));
    drop(guard);
}

#[test]
fn a_session_dropped_inside_a_transaction_is_released_when_it_ends() {
    let source = source();
    let writer = source.writer();
    writer.resize(2, 2).unwrap();
    let doomed = source.writer_exclusive();
    let fresh_token = source.writer();
    drop(writer); // revoked: its drop changes nothing
    let mut guard = fresh_token.lock().unwrap();
    guard.fill_rect(PixelRect::new(0, 0, 1, 1), [1; 4]).ok();
    // Drop every other token of the session while the transaction is open:
    // its release cannot lock, so the transaction applies it as it ends.
    drop(doomed);
    assert_eq!(source.stats().writers, 2, "not released yet");
    guard.commit();
    assert_eq!(source.stats().writers, 1);
    assert_eq!(
        source.status(),
        LiveImageStatus::Waiting,
        "a resize-free transaction on no buffer"
    );
    drop(fresh_token);
    assert_eq!(source.status(), LiveImageStatus::Disconnected);
}

#[test]
fn the_last_token_dropped_inside_its_own_transaction_disconnects_at_its_end() {
    let source = source();
    let writer = source.writer();
    writer.write_frame(2, 2, &[1; 16], 8).unwrap();
    let (waker, consumer) = attach(&source);
    {
        let clone = writer.clone();
        let guard = clone.lock().unwrap();
        drop(writer);
        // `clone` is the last holder of the token, and it is borrowed by the
        // guard: the token outlives the guard here, so nothing is deferred.
        drop(guard);
    }
    assert_eq!(source.status(), LiveImageStatus::Disconnected);
    assert!(consumer.take_geometry(), "the disconnect reached layout");
    assert!(waker.count_of(WakeKind::Layout) >= 1);
}

// ── A.14-A.16: the planner ──

#[test]
fn the_planner_goes_full_without_a_cursor_after_a_resize_or_a_clear() {
    let source = source();
    let writer = source.writer();
    writer.write_frame(32, 32, &frame(32, 32, 0), 128).unwrap();
    let (_, consumer) = attach(&source);
    let g = source.generation();
    assert_eq!(plan(&consumer, None), UploadPlan::Full, "a new texture");
    assert_eq!(plan(&consumer, Some(g)), UploadPlan::Nothing, "no lag");
    writer
        .write_rect(PixelRect::new(1, 1, 1, 1), &[0; 4], 4)
        .unwrap();
    writer.resize(16, 16).unwrap();
    writer
        .write_rect(PixelRect::new(1, 1, 1, 1), &[0; 4], 4)
        .unwrap();
    assert_eq!(
        plan(&consumer, Some(g)),
        UploadPlan::Full,
        "a resize in the range"
    );
    let g = source.generation();
    writer.clear().unwrap();
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[0; 4], 4)
        .unwrap_err();
    writer.write_frame(16, 16, &frame(16, 16, 1), 64).unwrap();
    assert_eq!(
        plan(&consumer, Some(g)),
        UploadPlan::Full,
        "a clear in the range"
    );
}

#[test]
fn the_planner_unions_rects_and_widens_past_its_limits() {
    let source = source();
    let writer = source.writer();
    writer
        .write_frame(100, 100, &frame(100, 100, 0), 400)
        .unwrap();
    let (_, consumer) = attach(&source);
    let g = source.generation();
    let one = |x: u32| {
        writer
            .write_rect(PixelRect::new(x, x, 2, 2), &[0; 16], 8)
            .unwrap()
    };
    one(1);
    one(10);
    one(10); // an exact duplicate is uploaded once
    one(20);
    assert_eq!(
        rects(plan(&consumer, Some(g))),
        vec![
            PixelRect::new(1, 1, 2, 2),
            PixelRect::new(10, 10, 2, 2),
            PixelRect::new(20, 20, 2, 2)
        ],
        "lag 4 is the union"
    );
    // Lag 16 is the whole log; lag 17 is past it.
    let g = source.generation();
    for x in 0..16 {
        one(x * 4);
    }
    assert_eq!(rects(plan(&consumer, Some(g))).len(), 16, "lag 16");
    one(70);
    assert_eq!(plan(&consumer, Some(g)), UploadPlan::Full, "lag 17");
    // 17 distinct rects over a lag the log holds.
    let g = source.generation();
    let mut guard = writer.lock().unwrap();
    for x in 0..9 {
        guard
            .write_rect(PixelRect::new(x * 5, 0, 1, 1), &[0; 4], 4)
            .unwrap();
    }
    guard.commit();
    let mut guard = writer.lock().unwrap();
    for x in 0..8 {
        guard
            .write_rect(PixelRect::new(x * 5, 50, 1, 1), &[0; 4], 4)
            .unwrap();
    }
    guard.commit();
    assert_eq!(plan(&consumer, Some(g)), UploadPlan::Full, "17 rects");
    // Half the frame, in two commits that overlap, is Full.
    let g = source.generation();
    writer
        .write_rect(PixelRect::new(0, 0, 100, 30), &[0; 12_000], 400)
        .unwrap();
    writer
        .write_rect(PixelRect::new(0, 25, 100, 25), &[0; 10_000], 400)
        .unwrap();
    assert_eq!(plan(&consumer, Some(g)), UploadPlan::Full, "50 % covered");
    // Just under half is still rects, overlaps counted once.
    let g = source.generation();
    writer
        .write_rect(PixelRect::new(0, 0, 100, 30), &[0; 12_000], 400)
        .unwrap();
    writer
        .write_rect(PixelRect::new(0, 10, 100, 39), &[0; 15_600], 400)
        .unwrap();
    assert_eq!(rects(plan(&consumer, Some(g))).len(), 2, "49 % covered");
}

#[test]
fn a_commit_of_more_than_sixteen_rects_records_their_bounding_box() {
    let source = source();
    let writer = source.writer();
    writer.write_frame(64, 64, &frame(64, 64, 0), 256).unwrap();
    let (_, consumer) = attach(&source);
    let g = source.generation();
    let mut guard = writer.lock().unwrap();
    for i in 0..17 {
        guard
            .write_rect(PixelRect::new(i, 2, 1, 1), &[0; 4], 4)
            .unwrap();
    }
    guard.commit();
    assert_eq!(
        rects(plan(&consumer, Some(g))),
        vec![PixelRect::new(0, 2, 17, 1)],
        "the 17th widens the commit to its bounding box"
    );
}

#[test]
fn two_cursors_get_independent_plans() {
    let source = source();
    let writer = source.writer();
    writer.write_frame(50, 50, &frame(50, 50, 0), 200).unwrap();
    let (_, consumer) = attach(&source);
    let a = source.generation();
    writer
        .write_rect(PixelRect::new(1, 1, 1, 1), &[0; 4], 4)
        .unwrap();
    let b = source.generation();
    writer
        .write_rect(PixelRect::new(30, 30, 1, 1), &[0; 4], 4)
        .unwrap();
    assert_eq!(rects(plan(&consumer, Some(a))).len(), 2);
    assert_eq!(
        rects(plan(&consumer, Some(b))),
        vec![PixelRect::new(30, 30, 1, 1)]
    );
}

#[test]
fn a_commit_instant_is_recorded_for_the_log() {
    let source = source();
    let writer = source.writer();
    let before = std::time::Instant::now();
    writer.write_frame(2, 2, &[0; 16], 8).unwrap();
    let (_, consumer) = attach(&source);
    let ReadAttempt::Locked(read) = consumer.try_read() else {
        panic!("locked")
    };
    let at = read.commit_instant(read.generation()).expect("recorded");
    assert!(at >= before);
    assert_eq!(read.commit_instant(read.generation() + 1), None);
}

// ── A.18: the wake flags and the handoff ──

#[test]
fn a_status_change_raises_only_the_geometry_flag_and_a_commit_the_pixel_flag() {
    let source = source();
    let (waker, consumer) = attach(&source);
    let writer = source.writer();
    assert!(
        consumer.take_geometry(),
        "Disconnected to Waiting is layout's"
    );
    assert!(!consumer.take_pixels());
    assert_eq!(waker.count_of(WakeKind::Layout), 1);
    writer.resize(4, 4).unwrap();
    assert!(consumer.take_geometry(), "a size change");
    assert!(consumer.take_pixels(), "and a commit");
    assert_eq!(
        waker.count_of(WakeKind::Layout),
        2,
        "both raised: one wake, the larger"
    );
    assert_eq!(waker.count_of(WakeKind::Draw), 0);
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[0; 4], 4)
        .unwrap();
    assert!(consumer.take_geometry(), "Waiting to Live");
    assert!(consumer.take_pixels());
    writer
        .write_rect(PixelRect::new(1, 0, 1, 1), &[0; 4], 4)
        .unwrap();
    assert!(!consumer.take_geometry(), "pixels alone");
    assert!(consumer.take_pixels());
    assert_eq!(waker.count_of(WakeKind::Draw), 1);
    assert_eq!(
        consumer.layout_meta(),
        LiveMeta::default(),
        "only the pre-pass records"
    );
}

#[test]
fn a_hundred_commits_between_renders_wake_once() {
    let source = source();
    let writer = source.writer();
    writer.write_frame(8, 8, &frame(8, 8, 0), 32).unwrap();
    let (waker, consumer) = attach(&source);
    for i in 0..100 {
        writer
            .write_rect(PixelRect::new(i % 8, 0, 1, 1), &[0; 4], 4)
            .unwrap();
    }
    assert_eq!(waker.count(), 1);
    assert_eq!(source.stats().wakes, 1);
    assert_eq!(source.stats().wakes_coalesced, 99);
    consumer.take_pixels();
    writer
        .write_rect(PixelRect::new(0, 1, 1, 1), &[0; 4], 4)
        .unwrap();
    assert_eq!(waker.count(), 2, "taken, so the next commit wakes again");
    consumer.park_pixels();
    consumer.take_pixels();
    consumer.park_pixels();
    writer
        .write_rect(PixelRect::new(0, 2, 1, 1), &[0; 4], 4)
        .unwrap();
    assert_eq!(waker.count(), 2, "parked: a paused window is not woken");
}

#[test]
fn two_attachments_in_one_window_wake_it_once() {
    let source = source();
    let writer = source.writer();
    writer.write_frame(4, 4, &frame(4, 4, 0), 16).unwrap();
    let waker = Arc::new(CountingWaker::new());
    let a = source.attach(Some(waker.clone() as Arc<dyn RedrawWaker>));
    let b = source.attach(Some(waker.clone() as Arc<dyn RedrawWaker>));
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[0; 4], 4)
        .unwrap();
    assert_eq!(waker.count(), 1);
    assert!(a.take_pixels() && b.take_pixels());
}

#[test]
fn a_busy_read_is_woken_by_the_unlock_that_follows_even_with_its_flags_raised() {
    let source = source();
    let writer = source.writer();
    writer.write_frame(4, 4, &frame(4, 4, 0), 16).unwrap();
    let (waker, consumer) = attach(&source);
    let (_, other) = attach(&source);
    let before = waker.count();
    let (held, release) = (
        Arc::new(std::sync::Barrier::new(2)),
        Arc::new(std::sync::Barrier::new(2)),
    );
    let holder = {
        let (writer, held, release) = (writer.clone(), held.clone(), release.clone());
        std::thread::spawn(move || {
            // Not a transaction: a session call holds the lock too.
            let guard = writer.lock().unwrap();
            held.wait();
            release.wait();
            drop(guard); // abandoned, empty: only the handoff wakes
        })
    };
    held.wait();
    assert!(matches!(consumer.try_read(), ReadAttempt::Busy));
    assert!(matches!(
        consumer.read_for(Duration::from_millis(5)),
        ReadAttempt::Busy
    ));
    assert_eq!(waker.count(), before);
    release.wait();
    holder.join().unwrap();
    assert_eq!(waker.count(), before + 1, "the unlock woke the busy reader");
    assert!(!other.shared.handoff.is_raised(), "and nobody else");
    // A successful retry clears the registration: the next unlock wakes no one.
    assert!(matches!(consumer.try_read(), ReadAttempt::Locked(_)));
    drop(writer.lock().unwrap());
    assert_eq!(waker.count(), before + 1);
}

#[test]
fn a_read_on_a_thread_holding_the_transaction_is_busy_and_woken_at_its_end() {
    let source = source();
    let writer = source.writer();
    writer.write_frame(4, 4, &frame(4, 4, 0), 16).unwrap();
    let (waker, consumer) = attach(&source);
    let before = waker.count();
    let guard = writer.lock().unwrap();
    // An async task suspended in a transaction on the UI thread: the read
    // must not wait for its own thread.
    let start = std::time::Instant::now();
    assert!(matches!(
        consumer.read_for(Duration::from_secs(5)),
        ReadAttempt::Busy
    ));
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "it waited for itself"
    );
    drop(guard);
    assert_eq!(waker.count(), before + 1);
}

#[test]
fn read_for_waits_for_the_holder() {
    let source = source();
    let writer = source.writer();
    writer.write_frame(4, 4, &frame(4, 4, 0), 16).unwrap();
    let (_, consumer) = attach(&source);
    let held = Arc::new(std::sync::Barrier::new(2));
    let holder = {
        let (writer, held) = (writer.clone(), held.clone());
        std::thread::spawn(move || {
            let mut guard = writer.lock().unwrap();
            held.wait();
            std::thread::sleep(Duration::from_millis(30));
            guard.fill_rect(PixelRect::full(4, 4), [7; 4]).unwrap();
            guard.commit()
        })
    };
    held.wait();
    let ReadAttempt::Locked(read) = consumer.read_for(Duration::from_secs(10)) else {
        panic!("the holder releases within 30 ms")
    };
    let committed = read.generation();
    assert_eq!(read.pixels()[0], 7, "it read the committed frame");
    drop(read);
    assert_eq!(holder.join().unwrap(), committed);
}

// ── A.19 (part), attach and detach ──

#[test]
fn attach_records_the_meta_and_detach_prunes() {
    let source = LiveImageSource::builder(LivePixelFormat::Rgba8)
        .size_hint(9, 7)
        .build();
    let writer = source.writer();
    let (_, consumer) = attach(&source);
    assert_eq!(
        consumer.layout_meta(),
        LiveMeta {
            status: LiveImageStatus::Waiting,
            size: Some((9, 7))
        },
        "recorded at attach, for a widget built after the pre-pass"
    );
    assert!(!consumer.take_geometry(), "its flags start clear");
    assert_eq!(source.stats().attachments, 1);
    consumer.set_observed(true);
    consumer.set_paused(true);
    assert!(source.is_observed());
    assert_eq!(source.stats().paused_attachments, 1);
    consumer.detach();
    consumer.detach();
    let stats = source.stats();
    assert_eq!(
        (
            stats.attachments,
            stats.observed_attachments,
            stats.paused_attachments
        ),
        (0, 0, 0)
    );
    assert!(!source.is_observed());
    consumer.set_observed(true);
    assert!(
        !source.is_observed(),
        "a detached consumer is never counted"
    );
    writer.write_frame(1, 1, &[0; 4], 4).unwrap();
    assert_eq!(
        source.shared.live_consumers().len(),
        0,
        "pruned at the commit"
    );
    let dropped = source.attach(None);
    dropped.set_observed(true);
    drop(dropped);
    assert_eq!(
        source.stats().attachments,
        0,
        "dropping the last clone detaches"
    );
    assert!(!source.is_observed());
}

#[test]
fn is_displayed_needs_an_unpaused_observed_attachment_in_a_shown_window() {
    struct Hidden(std::sync::atomic::AtomicBool);
    impl RedrawWaker for Hidden {
        fn wake(&self, _: WakeKind) {}
        fn is_hidden(&self) -> bool {
            self.0.load(std::sync::atomic::Ordering::Relaxed)
        }
    }
    let source = source();
    let window = Arc::new(Hidden(std::sync::atomic::AtomicBool::new(false)));
    let consumer = source.attach(Some(window.clone() as Arc<dyn RedrawWaker>));
    assert!(!source.is_displayed(), "not in a render yet");
    consumer.set_observed(true);
    assert!(source.is_displayed());
    consumer.set_paused(true);
    assert!(!source.is_displayed(), "paused");
    consumer.set_paused(false);
    window.0.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(!source.is_displayed(), "its window is hidden");
    assert!(source.is_observed(), "still observed");
}

#[test]
fn a_waker_installed_late_wakes_for_what_was_raised_without_one() {
    let source = source();
    let writer = source.writer();
    let consumer = source.attach(None);
    writer.write_frame(2, 2, &[0; 16], 8).unwrap();
    let waker = Arc::new(CountingWaker::new());
    consumer.set_waker(Some(waker.clone() as Arc<dyn RedrawWaker>));
    assert_eq!(
        waker.count_of(WakeKind::Layout),
        1,
        "the size change is owed"
    );
}

#[test]
fn render_records_count_into_the_attachment() {
    let source = source();
    let writer = source.writer();
    writer.write_frame(2, 2, &[0; 16], 8).unwrap();
    let (_, consumer) = attach(&source);
    consumer.record_render(RenderRecord {
        drawn: true,
        uploaded: true,
        window_generation: 1,
        ..Default::default()
    });
    consumer.record_render(RenderRecord {
        drawn: true,
        capture: true,
        uploaded: true,
        window_generation: 2,
        ..Default::default()
    });
    consumer.record_render(RenderRecord {
        drawn: true,
        paused: true,
        kept: true,
        window_generation: 2,
        ..Default::default()
    });
    consumer.count_paint();
    let s = consumer.stats();
    assert_eq!(
        s.attachment.frames_drawn, 2,
        "the capture is not a frame drawn"
    );
    assert_eq!(s.attachment.captures, 1);
    assert_eq!(s.attachment.uploads, 2);
    assert_eq!(s.attachment.paused_frames, 1);
    assert_eq!(s.attachment.window_generation, 2);
    assert_eq!(s.attachment.paints, 1);
    assert_eq!(
        s.source.displayed_generation, 2,
        "raised by the presented frame at 2, not by the capture alone"
    );
    let fresh = source.attach(None);
    fresh.record_render(RenderRecord {
        capture: true,
        drawn: true,
        window_generation: 9,
        ..Default::default()
    });
    assert_eq!(
        source.displayed_generation(),
        2,
        "a capture never raises it"
    );
}

// ── 11.4: stress ──

/// A consumer that takes its flag, plans, copies what the plan names and
/// waits for the next wake, against four producers committing 10 000 times
/// each: it reaches the last generation with the source's pixels.
#[test]
fn a_consumer_behind_four_fast_producers_converges_on_their_last_frame() {
    const W: u32 = 64;
    const H: u32 = 64;
    const PER: u32 = 10_000;
    let source = source();
    let writer = source.writer();
    writer
        .write_frame(W, H, &frame(W, H, 0), (W * 4) as usize)
        .unwrap();
    let (waker, consumer) = attach(&source);
    let mut texture = vec![0u8; (W * H * 4) as usize];
    let mut cursor: Option<u64> = None;
    let producers: Vec<_> = (0..4u32)
        .map(|t| {
            let writer = writer.clone();
            std::thread::spawn(move || {
                for i in 0..PER {
                    let x = (i * 7 + t * 13) % W;
                    let y = (i * 3 + t * 29) % H;
                    let px = [t as u8, (i >> 8) as u8, i as u8, 255];
                    let mut guard = writer.lock().unwrap();
                    guard.fill_rect(PixelRect::new(x, y, 1, 1), px).unwrap();
                    guard.commit();
                }
            })
        })
        .collect();
    loop {
        let wakes = waker.count();
        consumer.take_pixels();
        let done = producers.iter().all(|p| p.is_finished());
        if let ReadAttempt::Locked(read) = consumer.try_read() {
            let stride = read.stride();
            let copy = |texture: &mut Vec<u8>, r: PixelRect| {
                for y in r.y..r.y + r.height {
                    let at = y as usize * stride + r.x as usize * 4;
                    let len = r.width as usize * 4;
                    texture[at..at + len].copy_from_slice(&read.pixels()[at..at + len]);
                }
            };
            match read.plan(cursor) {
                UploadPlan::Nothing => {}
                UploadPlan::Full => copy(&mut texture, PixelRect::full(W, H)),
                UploadPlan::Rects(rects) => {
                    for &r in rects.as_slice() {
                        copy(&mut texture, r);
                    }
                }
            }
            cursor = Some(read.generation());
        }
        if done && cursor == Some(source.generation()) {
            break;
        }
        // A commit after the take wakes; the timeout only ends a wait that
        // began after the producers' last one.
        waker.wait_for(wakes + 1, Duration::from_millis(50));
    }
    for p in producers {
        p.join().unwrap();
    }
    let (_, final_pixels, generation) = pixels(&consumer);
    assert_eq!(generation, 1 + 4 * PER as u64);
    assert_eq!(texture, final_pixels, "the mirrored texture is the source");
}

/// Two threads alternate dropping the last writer and taking a new one,
/// resizing and writing: no live writer ever finds no frame, and the status
/// always matches the writer count.
#[test]
fn the_writer_count_never_races_the_last_writers_drop() {
    let source = source();
    let iterations = 50_000;
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let source = source.clone();
            std::thread::spawn(move || {
                for i in 0..iterations {
                    let writer = source.writer();
                    writer.resize(4 + (i % 3), 4).unwrap();
                    writer
                        .write_rect(PixelRect::new(0, 0, 1, 1), &[1; 4], 4)
                        .expect("a live writer always has a frame after its resize");
                    let status = source.status();
                    assert_ne!(status, LiveImageStatus::Disconnected, "a writer is alive");
                    drop(writer);
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(source.status(), LiveImageStatus::Disconnected);
    assert_eq!(source.stats().writers, 0);
}
