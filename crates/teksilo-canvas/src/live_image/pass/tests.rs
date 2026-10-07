// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The live pass, through the mirror: every row of the upload table (spec
//! 6.1, 6.3), the decision table, release and the parked pool, the pause
//! rule, capture mode, the staging budget's spread fills, and the frame
//! plumbing (spec A.17, A.19, C.17).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::super::testing::LiveImageMirror;
use super::super::*;
use super::PARK_MAX_TEXTURE;
use crate::geometry::{Point, Rect};
use crate::wake::{CountingWaker, RedrawWaker, WakeKind};
use crate::{Canvas, DrawCommand, RenderFrame};

/// A `w × h` frame whose pixel `(x, y)` is `[x, y, seed, 255]` (wrapped).
fn pixels(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            px.extend_from_slice(&[x as u8, y as u8, seed, 255]);
        }
    }
    px
}

/// A live source of `w × h` and its writer.
fn live(w: u32, h: u32) -> (LiveImageSource, LiveImageWriter) {
    let source = LiveImageSource::builder(LivePixelFormat::Rgba8)
        .label("pass")
        .build();
    let writer = source.writer();
    writer
        .write_frame(w, h, &pixels(w, h, 0), (w * 4) as usize)
        .unwrap();
    (source, writer)
}

/// A widget showing `source` in a window counted by the returned waker.
fn widget(source: &LiveImageSource) -> (Arc<CountingWaker>, LiveImageConsumer) {
    let waker = Arc::new(CountingWaker::new());
    let consumer = source.attach(Some(waker.clone() as Arc<dyn RedrawWaker>));
    (waker, consumer)
}

/// The layout pre-pass, for one consumer.
fn lay_out(consumer: &LiveImageConsumer) {
    let _ = consumer.take_geometry();
    consumer.record_layout_meta(consumer.source().meta());
}

fn draw() -> LiveImageDraw {
    LiveImageDraw::new(
        Rect::new(0.0, 0.0, 64.0, 64.0),
        Rect::new(0.0, 0.0, 64.0, 64.0),
    )
}

/// A frame drawing each consumer once, laid out first.
fn frame(consumers: &[&LiveImageConsumer]) -> RenderFrame {
    frame_with(consumers, draw())
}

fn frame_with(consumers: &[&LiveImageConsumer], d: LiveImageDraw) -> RenderFrame {
    let mut canvas = Canvas::new();
    for consumer in consumers {
        lay_out(consumer);
        canvas.draw_live_image(consumer, &d);
    }
    canvas.into_render_frame()
}

fn texture(mirror: &LiveImageMirror, source: &LiveImageSource) -> Vec<u8> {
    mirror
        .pixels(source.id())
        .map(|(_, _, p)| p.to_vec())
        .expect("a texture")
}

fn current(source: &LiveImageSource) -> Vec<u8> {
    let (_, consumer) = widget(source);
    let read = match consumer.try_read() {
        super::super::internal::ReadAttempt::Locked(read) => read,
        super::super::internal::ReadAttempt::Busy => panic!("locked"),
    };
    let (w, h) = read.size().expect("a buffer");
    let mut tight = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h as usize {
        tight.extend_from_slice(
            &read.pixels()[y * read.stride()..y * read.stride() + w as usize * 4],
        );
    }
    tight
}

// ── first frame, pixel commits, idle ──

#[test]
fn the_first_frame_uploads_whole_and_draws_it() {
    let (source, _writer) = live(8, 6);
    let (_, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    let report = mirror.consume(&frame(&[&consumer]));
    assert_eq!((report.full_uploads, report.partial_uploads), (1, 0));
    assert_eq!(report.bytes, 8 * 6 * 4);
    assert_eq!(texture(&mirror, &source), pixels(8, 6, 0));
    let d = mirror.decisions()[0];
    assert!(d.draw && !d.deferred);
    assert_eq!(d.generation, source.generation());
    let s = consumer.stats().attachment;
    assert_eq!((s.frames_drawn, s.uploads, s.window_generation), (1, 1, 1));
    assert!(s.observed);
    assert_eq!(source.displayed_generation(), 1);
}

#[test]
fn a_pixel_commit_uploads_only_its_rects_and_an_idle_frame_nothing() {
    let (source, writer) = live(16, 16);
    let (waker, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame(&[&consumer]));
    writer
        .write_rect(PixelRect::new(2, 3, 2, 2), &[9; 16], 8)
        .unwrap();
    assert_eq!(waker.count_of(WakeKind::Draw), 1);
    let report = mirror.consume(&frame(&[&consumer]));
    assert_eq!((report.full_uploads, report.partial_uploads), (0, 1));
    assert_eq!(report.rects, vec![PixelRect::new(2, 3, 2, 2)]);
    assert_eq!(report.bytes, 2 * 2 * 4);
    assert_eq!(texture(&mirror, &source), current(&source));
    // Idle: nothing to upload, and the lock is not taken: a producer holding
    // it does not make the frame busy.
    let guard = writer.lock().unwrap();
    let idle = mirror.consume(&frame(&[&consumer]));
    assert_eq!(idle, Default::default());
    drop(guard);
    assert_eq!(mirror.stats().contended, 0);
}

#[test]
fn a_frame_that_has_not_seen_a_commit_wakes_again_after_it() {
    let (source, writer) = live(4, 4);
    let (waker, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame(&[&consumer]));
    for _ in 0..100 {
        writer
            .write_rect(PixelRect::new(0, 0, 1, 1), &[1; 4], 4)
            .unwrap();
    }
    assert_eq!(waker.count(), 1, "a hundred commits, one wake");
    mirror.consume(&frame(&[&consumer]));
    writer
        .write_rect(PixelRect::new(1, 0, 1, 1), &[1; 4], 4)
        .unwrap();
    assert_eq!(
        waker.count(),
        2,
        "the frame took the flag: the next commit wakes"
    );
}

// ── A.17: a source larger than the device ──

#[test]
fn a_source_larger_than_the_device_draws_background_and_wakes_no_more() {
    let (source, writer) = live(65, 3);
    let (waker, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.set_max_dimension(64);
    let report = mirror.consume(&frame(&[&consumer]));
    assert_eq!(report.full_uploads, 0);
    assert_eq!(mirror.texture_count(), 0);
    let d = mirror.decisions()[0];
    assert!(!d.draw && d.deferred);
    let before = waker.count();
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[1; 4], 4)
        .unwrap();
    assert_eq!(
        waker.count(),
        before,
        "its pixel flag is parked: no redraw per commit"
    );
}

// ── busy, stale, the blocking read ──

#[test]
fn a_busy_lock_draws_the_held_texture_and_its_unlock_wakes() {
    let (source, writer) = live(4, 4);
    let (waker, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame(&[&consumer]));
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[7; 4], 4)
        .unwrap();
    let before = waker.count();
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
    let report = mirror.consume(&frame(&[&consumer]));
    assert_eq!(report.contended, 1);
    assert_eq!(report.deferred, 1);
    assert!(mirror.decisions()[0].draw, "the held texture still draws");
    assert_eq!(consumer.stats().attachment.deferred_frames, 1);
    release.wait();
    holder.join().unwrap();
    assert_eq!(waker.count(), before + 1, "the unlock woke the window");
    let report = mirror.consume(&frame(&[&consumer]));
    assert_eq!(report.partial_uploads, 1);
    assert_eq!(texture(&mirror, &source), current(&source));
}

#[test]
fn two_busy_frames_in_a_row_make_the_third_wait_for_the_lock() {
    let (source, writer) = live(4, 4);
    let (_, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame(&[&consumer]));
    let committed = Arc::new(std::sync::Barrier::new(2));
    let (go, done) = (
        std::sync::mpsc::channel::<()>(),
        std::sync::mpsc::channel::<()>(),
    );
    // Something new for every frame below, so each one takes the lock.
    writer
        .write_rect(PixelRect::new(2, 2, 1, 1), &[1; 4], 4)
        .unwrap();
    let producer = {
        let (writer, committed) = (writer.clone(), committed.clone());
        let (go, done) = (go.1, done.0);
        std::thread::spawn(move || {
            // Re-lock at once after every commit: the starving producer.
            for _ in 0..2 {
                let mut guard = writer.lock().unwrap();
                guard.fill_rect(PixelRect::new(0, 0, 1, 1), [3; 4]).unwrap();
                committed.wait();
                go.recv().unwrap();
                guard.commit();
            }
            let mut guard = writer.lock().unwrap();
            guard.fill_rect(PixelRect::new(1, 1, 1, 1), [4; 4]).unwrap();
            committed.wait();
            std::thread::sleep(Duration::from_millis(5));
            guard.commit();
            done.send(()).unwrap();
        })
    };
    for _ in 0..2 {
        committed.wait();
        mirror.consume(&frame(&[&consumer]));
        go.0.send(()).unwrap();
    }
    assert_eq!(mirror.stats().contended, 2);
    let waits = mirror.stats().blocking_waits;
    committed.wait();
    let report = mirror.consume(&frame(&[&consumer]));
    assert_eq!(
        mirror.stats().blocking_waits,
        waits + 1,
        "the third frame waited"
    );
    assert_eq!(report.contended, 0);
    assert!(report.partial_uploads + report.full_uploads == 1);
    done.1.recv().unwrap();
    producer.join().unwrap();
}

#[test]
fn a_size_changed_after_layout_draws_the_held_texture() {
    let (source, writer) = live(4, 4);
    let (_, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame(&[&consumer]));
    // Laid out at 4 × 4; the producer resizes before the live pass.
    let mut canvas = Canvas::new();
    lay_out(&consumer);
    canvas.draw_live_image(&consumer, &draw());
    let f = canvas.into_render_frame();
    writer.write_frame(5, 4, &pixels(5, 4, 1), 20).unwrap();
    let report = mirror.consume(&f);
    assert_eq!(mirror.stats().stale_deferrals, 1);
    assert_eq!(report.deferred, 1);
    assert!(mirror.decisions()[0].draw, "the 4 × 4 texture still draws");
    assert_eq!(mirror.pixels(source.id()).unwrap().0, 4);
    // The next layout sees 5 × 4: a staged texture, filled whole.
    let report = mirror.consume(&frame(&[&consumer]));
    assert_eq!(report.full_uploads, 1);
    assert_eq!(texture(&mirror, &source), pixels(5, 4, 1));
    assert_eq!(
        mirror.texture_count(),
        1,
        "the old texture went with the swap"
    );
}

/// A resize staged a texture whose fill met a producer's transaction, and
/// the size came back to the current texture's before the fill ran. The
/// staged texture is of the size in between: it must go, not take the
/// frame. Filled anyway, the mirror wrote out of its bounds, and the GPU
/// issued an out-of-bounds texture write.
#[test]
fn a_texture_staged_for_a_size_that_went_back_is_dropped_not_filled() {
    let (source, writer) = live(8, 8);
    let (_, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame(&[&consumer]));
    writer.write_frame(4, 4, &pixels(4, 4, 1), 16).unwrap();

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
    let report = mirror.consume(&frame(&[&consumer]));
    assert_eq!(report.deferred, 1, "the 4 x 4 frame met the transaction");
    assert_eq!(
        mirror.texture_count(),
        2,
        "precondition: the 8 x 8 texture and a staged 4 x 4 one"
    );
    release.wait();
    holder.join().unwrap();

    writer.write_frame(8, 8, &pixels(8, 8, 2), 32).unwrap();
    let report = mirror.consume(&frame(&[&consumer]));
    assert_eq!(report.deferred, 0);
    assert_eq!(texture(&mirror, &source), pixels(8, 8, 2));
    assert_eq!(
        mirror.texture_count(),
        1,
        "the staged 4 x 4 texture is gone, released"
    );
}

// ── timings ──

/// What the last frame timed: lock holds and upload delays, as counts.
fn timed(mirror: &LiveImageMirror) -> (usize, usize) {
    let t = mirror.last_timings();
    (t.lock_holds.len(), t.commit_to_upload.len())
}

/// A lock taken is a hold timed, and an upload completed is a delay timed
/// from its commit; a frame that takes no lock times nothing, and each frame
/// times only its own.
#[test]
fn each_lock_hold_and_each_completed_upload_is_timed() {
    let (source, writer) = live(16, 16);
    let (_, consumer) = widget(&source);
    let (other_source, _other_writer) = live(4, 4);
    let (_, other) = widget(&other_source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame(&[&consumer, &other]));
    assert_eq!(
        timed(&mirror),
        (2, 2),
        "two sources, each locked and uploaded"
    );

    mirror.consume(&frame(&[&consumer, &other]));
    assert_eq!(timed(&mirror), (0, 0), "nothing new: no lock taken");

    writer
        .write_rect(PixelRect::new(0, 0, 2, 2), &[5; 16], 8)
        .unwrap();
    std::thread::sleep(Duration::from_millis(20));
    mirror.consume(&frame(&[&consumer, &other]));
    assert_eq!(timed(&mirror), (1, 1));
    assert!(
        mirror.last_timings().commit_to_upload[0] >= Duration::from_millis(20),
        "from the commit, not from the frame: {:?}",
        mirror.last_timings()
    );

    // A size changed after layout: locked, nothing uploaded.
    let mut canvas = Canvas::new();
    lay_out(&consumer);
    canvas.draw_live_image(&consumer, &draw());
    let stale = canvas.into_render_frame();
    writer.write_frame(16, 20, &pixels(16, 20, 1), 64).unwrap();
    mirror.consume(&stale);
    assert_eq!(
        timed(&mirror),
        (1, 0),
        "a stale size is a hold without an upload"
    );

    // Filled five rows a frame: each partial frame holds the lock and
    // uploads nothing whole; the last completes the upload.
    mirror.set_staging_budget(16 * 4 * 5);
    let mut partial = 0;
    loop {
        let report = mirror.consume(&frame(&[&consumer]));
        if report.full_uploads == 1 {
            assert_eq!(timed(&mirror), (1, 1), "the completing frame");
            break;
        }
        assert_eq!(timed(&mirror), (1, 0), "a partial frame");
        partial += 1;
        assert!(partial < 10, "the fill makes progress");
    }
    assert_eq!(partial, 3);

    // A whole new frame of the same size, above the budget: the frame that
    // finds it holds the lock, uploads nothing and stages a second texture.
    writer.write_frame(16, 20, &pixels(16, 20, 2), 64).unwrap();
    mirror.consume(&frame(&[&consumer]));
    assert_eq!(timed(&mirror), (1, 0), "a plan too large for one frame");
}

/// A busy lock is neither a hold nor an upload.
#[test]
fn a_busy_lock_times_nothing() {
    let (source, writer) = live(4, 4);
    let (_, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame(&[&consumer]));
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[7; 4], 4)
        .unwrap();
    let guard = writer.lock().unwrap();
    let report = mirror.consume(&frame(&[&consumer]));
    assert_eq!(report.contended, 1);
    assert_eq!(timed(&mirror), (0, 0));
    drop(guard);
}

// ── release and the parked pool ──

#[test]
fn a_frame_without_the_source_frees_a_streaming_texture() {
    let (source, writer) = live(4, 4);
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[1; 4], 4)
        .unwrap();
    let (_, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame(&[&consumer]));
    assert_eq!(mirror.texture_count(), 1);
    let report = mirror.consume(&RenderFrame::new());
    assert_eq!(report.pruned, 1);
    let s = mirror.stats();
    assert_eq!(
        (s.textures, s.textures_parked, s.bytes, s.bytes_parked),
        (0, 0, 0, 0)
    );
    assert!(!source.is_observed(), "out of the frame: unobserved");
}

#[test]
fn a_small_one_commit_texture_parks_and_comes_back_without_an_upload() {
    let (source, _writer) = live(8, 8);
    let (_, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame(&[&consumer]));
    mirror.consume(&RenderFrame::new());
    let s = mirror.stats();
    assert_eq!(
        (s.textures, s.textures_parked, s.bytes_parked),
        (0, 1, 8 * 8 * 4)
    );
    let report = mirror.consume(&frame(&[&consumer]));
    assert_eq!(
        report.full_uploads + report.partial_uploads,
        0,
        "reused, not uploaded"
    );
    assert_eq!(texture(&mirror, &source), pixels(8, 8, 0));
    assert_eq!(mirror.stats().textures_parked, 0);
}

#[test]
fn the_park_budget_of_zero_frees_at_once_and_a_dead_source_frees_its_texture() {
    let (source, writer) = live(8, 8);
    let (_, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.set_park_budget(0);
    mirror.consume(&frame(&[&consumer]));
    mirror.consume(&RenderFrame::new());
    assert_eq!(mirror.stats().textures_parked, 0, "strict release");

    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame(&[&consumer]));
    mirror.consume(&RenderFrame::new());
    assert_eq!(mirror.stats().textures_parked, 1);
    drop((consumer, writer, source));
    mirror.consume(&RenderFrame::new());
    assert_eq!(mirror.stats().textures_parked, 0, "its source is gone");
}

#[test]
fn the_pool_parks_only_small_textures_and_evicts_the_oldest_past_its_budget() {
    let mut mirror = LiveImageMirror::new();
    // Over 4 MiB: freed, not parked.
    let side = ((PARK_MAX_TEXTURE / 4) as f64).sqrt() as u32 + 1;
    let (big, _bw) = live(side, side);
    let (_, big_c) = widget(&big);
    mirror.consume(&frame(&[&big_c]));
    mirror.consume(&RenderFrame::new());
    assert_eq!(mirror.stats().textures_parked, 0);
    // Five of 4 MiB each: the pool keeps the last four (16 MiB).
    let sources: Vec<_> = (0..5).map(|_| live(1024, 1024)).collect();
    for (source, _) in &sources {
        let (_, c) = widget(source);
        mirror.consume(&frame(&[&c]));
        mirror.consume(&RenderFrame::new());
    }
    let s = mirror.stats();
    assert_eq!(s.textures_parked, 4);
    assert_eq!(s.bytes_parked, 4 * 1024 * 1024 * 4);
    let (_, first) = widget(&sources[0].0);
    let report = mirror.consume(&frame(&[&first]));
    assert_eq!(report.full_uploads, 1, "the oldest was evicted");
}

// ── the pause rule ──

#[test]
fn a_paused_source_keeps_its_texture_and_parks_its_flag() {
    let (source, writer) = live(4, 4);
    let (waker, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame(&[&consumer]));
    let paused = draw().paused(true);
    let report = mirror.consume(&frame_with(&[&consumer], paused));
    assert_eq!(report.paused, 1);
    let before = waker.count();
    for _ in 0..100 {
        writer
            .write_rect(PixelRect::new(0, 0, 1, 1), &[5; 4], 4)
            .unwrap();
    }
    assert_eq!(waker.count(), before, "a paused window is not woken");
    let report = mirror.consume(&frame_with(&[&consumer], paused));
    assert_eq!(report.full_uploads + report.partial_uploads, 0);
    assert_eq!(texture(&mirror, &source), pixels(4, 4, 0), "the held frame");
    let s = consumer.stats();
    assert!(s.attachment.paused);
    assert_eq!(s.attachment.paused_frames, 2);
    assert_eq!(s.source.paused_attachments, 1);
    assert!(!source.is_displayed());
    // Reactivated: exactly one upload of the latest frame, whole after a
    // hundred commits.
    let report = mirror.consume(&frame(&[&consumer]));
    assert_eq!((report.full_uploads, report.partial_uploads), (1, 0));
    assert_eq!(texture(&mirror, &source), current(&source));
    assert!(!consumer.stats().attachment.paused);
}

#[test]
fn one_live_widget_keeps_a_shared_source_uploading() {
    let (source, writer) = live(4, 4);
    let waker = Arc::new(CountingWaker::new());
    let a = source.attach(Some(waker.clone() as Arc<dyn RedrawWaker>));
    let b = source.attach(Some(waker.clone() as Arc<dyn RedrawWaker>));
    let mut mirror = LiveImageMirror::new();
    let mixed = |mirror: &mut LiveImageMirror| {
        let mut canvas = Canvas::new();
        lay_out(&a);
        lay_out(&b);
        canvas.draw_live_image(&a, &draw().paused(true));
        canvas.draw_live_image(&b, &draw());
        mirror.consume(&canvas.into_render_frame())
    };
    assert_eq!(mixed(&mut mirror).full_uploads, 1);
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[5; 4], 4)
        .unwrap();
    let report = mixed(&mut mirror);
    assert_eq!(report.partial_uploads, 1, "one texture, one upload");
    assert_eq!(report.paused, 0);
    assert_eq!(mirror.texture_count(), 1);
    for c in [&a, &b] {
        let s = c.stats().attachment;
        assert!(!s.paused);
        assert_eq!(s.paused_frames, 0);
    }
}

#[test]
fn a_pause_never_shows_background_where_a_picture_can_be() {
    let (source, writer) = live(4, 4);
    let (_, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    let paused = draw().paused(true);
    let report = mirror.consume(&frame_with(&[&consumer], paused));
    assert_eq!(
        report.full_uploads, 1,
        "mounted paused: the first frame still shows"
    );
    writer.write_frame(6, 4, &pixels(6, 4, 2), 24).unwrap();
    let report = mirror.consume(&frame_with(&[&consumer], paused));
    assert_eq!(report.full_uploads, 1, "a resize while paused uploads once");
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[1; 4], 4)
        .unwrap();
    let report = mirror.consume(&frame_with(&[&consumer], paused));
    assert_eq!(
        report.full_uploads + report.partial_uploads,
        0,
        "then holds"
    );
    writer.clear().unwrap();
    let report = mirror.consume(&frame_with(&[&consumer], paused));
    assert_eq!(report.pruned, 1, "cleared: the texture goes");
    assert_eq!(mirror.texture_count(), 0);
}

// ── capture mode ──

#[test]
fn a_capture_uploads_through_a_pause_and_counts_as_a_capture() {
    let (source, writer) = live(4, 4);
    let (_, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    let paused = draw().paused(true);
    mirror.consume(&frame_with(&[&consumer], paused));
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[8; 4], 4)
        .unwrap();
    let displayed = source.displayed_generation();
    let report = mirror.capture(&frame_with(&[&consumer], paused));
    assert_eq!(report.partial_uploads, 1);
    assert_eq!(texture(&mirror, &source), current(&source));
    let s = consumer.stats();
    assert_eq!(s.attachment.captures, 1);
    assert_eq!(
        s.attachment.frames_drawn, 1,
        "the capture is no frame drawn"
    );
    assert!(s.attachment.paused, "still paused");
    assert_eq!(s.attachment.window_generation, source.generation());
    assert_eq!(
        source.displayed_generation(),
        displayed,
        "a capture displays nothing"
    );
}

#[test]
fn a_capture_waits_for_a_held_lock() {
    let (source, writer) = live(4, 4);
    let (_, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame(&[&consumer]));
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[1; 4], 4)
        .unwrap();
    let held = Arc::new(std::sync::Barrier::new(2));
    let holder = {
        let (writer, held) = (writer.clone(), held.clone());
        std::thread::spawn(move || {
            let mut guard = writer.lock().unwrap();
            held.wait();
            std::thread::sleep(Duration::from_millis(50));
            guard.fill_rect(PixelRect::new(3, 3, 1, 1), [9; 4]).unwrap();
            guard.commit()
        })
    };
    held.wait();
    let waits = mirror.stats().blocking_waits;
    let report = mirror.capture(&frame(&[&consumer]));
    let committed = holder.join().unwrap();
    assert_eq!(mirror.stats().blocking_waits, waits + 1);
    assert_eq!(report.contended, 0, "no busy row in a capture");
    assert_eq!(mirror.decisions()[0].generation, committed);
    assert_eq!(texture(&mirror, &source), current(&source));
}

// ── allocation failure and a lost device ──

#[test]
fn a_refused_texture_draws_background_and_retries_after_a_second_or_a_resize() {
    let (source, writer) = live(4, 4);
    let (waker, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.fail_next_texture();
    let t0 = Instant::now();
    let f = frame(&[&consumer]);
    mirror.consume_at(&f, t0);
    assert_eq!(mirror.stats().alloc_failures, 1);
    assert!(!mirror.decisions()[0].draw);
    let before = waker.count();
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[1; 4], 4)
        .unwrap();
    assert_eq!(waker.count(), before, "parked: no redraw per commit");
    mirror.consume_at(&frame(&[&consumer]), t0 + Duration::from_millis(500));
    assert_eq!(mirror.texture_count(), 0, "within the backoff, not retried");
    mirror.consume_at(&frame(&[&consumer]), t0 + Duration::from_secs(1));
    assert_eq!(mirror.texture_count(), 1, "retried after a second");
    // A size change retries at once.
    mirror.fail_next_texture();
    writer.write_frame(5, 5, &pixels(5, 5, 0), 20).unwrap();
    mirror.consume_at(&frame(&[&consumer]), t0 + Duration::from_secs(2));
    assert_eq!(mirror.stats().alloc_failures, 2);
    writer.write_frame(6, 6, &pixels(6, 6, 0), 24).unwrap();
    mirror.consume_at(&frame(&[&consumer]), t0 + Duration::from_secs(2));
    assert_eq!(texture(&mirror, &source), pixels(6, 6, 0));
}

#[test]
fn a_lost_device_stops_the_counters_and_parks_the_flags() {
    let (source, writer) = live(4, 4);
    let (waker, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame(&[&consumer]));
    mirror.set_device_lost(true);
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[1; 4], 4)
        .unwrap();
    let before = consumer.stats().attachment;
    let report = mirror.consume(&frame(&[&consumer]));
    assert_eq!(report.partial_uploads, 0);
    assert!(mirror.stats().device_lost);
    let after = consumer.stats().attachment;
    assert_eq!(
        (after.frames_drawn, after.uploads, after.window_generation),
        (
            before.frames_drawn,
            before.uploads,
            before.window_generation
        ),
        "nothing advances: a probe sees the stream stall"
    );
    let wakes = waker.count();
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[2; 4], 4)
        .unwrap();
    assert_eq!(
        waker.count(),
        wakes,
        "and the window is not woken per commit"
    );
}

// ── the staging budget ──

#[test]
fn a_whole_frame_above_the_budget_fills_over_several_frames() {
    let (source, writer) = live(16, 16);
    let (waker, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame(&[&consumer]));
    mirror.set_staging_budget(16 * 4 * 5); // five rows a frame
    // A resize: a staged texture, filled five rows at a time.
    writer.write_frame(16, 20, &pixels(16, 20, 3), 64).unwrap();
    let mut frames = 0;
    loop {
        let before = waker.count();
        let report = mirror.consume(&frame(&[&consumer]));
        frames += 1;
        if report.full_uploads == 1 {
            break;
        }
        assert_eq!(
            waker.count(),
            before + 1,
            "each partial frame asks for the next"
        );
        let d = mirror.decisions()[0];
        assert!(
            !d.draw && d.deferred,
            "no texture of the new size shows half-filled"
        );
        // A commit during the fill reaches the finished texture too.
        if frames == 2 {
            writer
                .write_rect(PixelRect::new(1, 1, 1, 1), &[0xEE; 4], 4)
                .unwrap();
        }
        assert!(frames < 10, "the fill makes progress");
    }
    assert_eq!(frames, 4, "twenty rows, five a frame");
    assert_eq!(texture(&mirror, &source), current(&source));
    assert!(mirror.decisions()[0].draw);
    assert_eq!(mirror.texture_count(), 1);
    assert_eq!(mirror.stats().progressive_bands, 4);
}

#[test]
fn a_plan_above_the_budget_fills_a_second_texture_and_never_tears() {
    let (source, writer) = live(16, 16);
    let (_, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame(&[&consumer]));
    let before = texture(&mirror, &source);
    mirror.set_staging_budget(16 * 4 * 4);
    writer.write_frame(16, 16, &pixels(16, 16, 9), 64).unwrap();
    let mut frames = 0;
    loop {
        let report = mirror.consume(&frame(&[&consumer]));
        frames += 1;
        if report.full_uploads == 1 {
            break;
        }
        assert_eq!(texture(&mirror, &source), before, "the old picture, whole");
        assert!(frames < 10);
    }
    assert_eq!(texture(&mirror, &source), pixels(16, 16, 9));
    assert_eq!(
        mirror.texture_count(),
        1,
        "the old texture went at the swap"
    );
}

#[test]
fn no_room_for_a_second_texture_uploads_in_place_on_the_next_frame() {
    let (source, writer) = live(16, 16);
    let (_, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame(&[&consumer]));
    mirror.set_staging_budget(64);
    writer.write_frame(16, 16, &pixels(16, 16, 4), 64).unwrap();
    mirror.fail_next_texture();
    mirror.consume(&frame(&[&consumer]));
    assert_eq!(mirror.stats().alloc_failures, 1);
    let report = mirror.consume(&frame(&[&consumer]));
    assert_eq!(report.full_uploads, 1);
    assert_eq!(texture(&mirror, &source), pixels(16, 16, 4));
}

#[test]
fn a_capture_completes_a_fill_in_progress() {
    let (source, writer) = live(8, 8);
    let (_, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.set_staging_budget(8 * 4 * 2);
    writer.write_frame(8, 9, &pixels(8, 9, 5), 32).unwrap();
    mirror.consume(&frame(&[&consumer]));
    assert_eq!(mirror.stats().uploads_full, 0, "spread");
    let report = mirror.capture(&frame(&[&consumer]));
    assert_eq!(report.full_uploads, 1, "every band before the draw");
    assert_eq!(texture(&mirror, &source), pixels(8, 9, 5));
}

#[test]
fn a_fill_whose_damage_grows_to_the_whole_frame_restarts_then_completes() {
    let (source, writer) = live(8, 8);
    let (_, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.set_staging_budget(8 * 4 * 4); // two frames per fill
    writer.write_frame(8, 10, &pixels(8, 10, 1), 32).unwrap();
    let mut seed = 1;
    let mut frames = 0;
    while mirror.stats().uploads_full == 0 {
        mirror.consume(&frame(&[&consumer]));
        // Rewrite the whole frame every frame: the damage since a fill's
        // first band is always Full.
        seed += 1;
        writer.write_frame(8, 10, &pixels(8, 10, seed), 32).unwrap();
        frames += 1;
        assert!(
            frames < 30,
            "a fill must end even against a full-frame producer"
        );
    }
    // The texture holds one whole generation, never a mix.
    let shown = texture(&mirror, &source);
    let g = shown[2];
    assert!(
        shown.chunks(4).all(|p| p[2] == g),
        "one generation throughout"
    );
}

#[test]
fn bands_cut_a_large_upload_without_changing_its_pixels() {
    let (source, writer) = live(37, 23);
    let (_, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.set_band_bytes(37 * 4 * 5);
    let report = mirror.consume(&frame(&[&consumer]));
    assert_eq!(report.rects.len(), 5, "23 rows, five a band");
    assert_eq!(mirror.stats().upload_calls, 5);
    assert_eq!(texture(&mirror, &source), pixels(37, 23, 0));
    writer
        .write_rect(PixelRect::new(3, 2, 10, 12), &vec![0xAA; 10 * 12 * 4], 40)
        .unwrap();
    let report = mirror.consume(&frame(&[&consumer]));
    assert_eq!(
        report.rects,
        vec![PixelRect::new(3, 2, 10, 12)],
        "a narrower rect fits more rows in a band: 12 rows of 40 bytes, one"
    );
    assert_eq!(texture(&mirror, &source), current(&source));
}

// ── one source, several quads; sizes; observation ──

#[test]
fn two_widgets_of_one_source_share_a_texture_and_an_upload() {
    let (source, writer) = live(4, 4);
    let waker = Arc::new(CountingWaker::new());
    let a = source.attach(Some(waker.clone() as Arc<dyn RedrawWaker>));
    let b = source.attach(Some(waker.clone() as Arc<dyn RedrawWaker>));
    let mut mirror = LiveImageMirror::new();
    assert_eq!(mirror.consume(&frame(&[&a, &b])).full_uploads, 1);
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[1; 4], 4)
        .unwrap();
    assert_eq!(mirror.consume(&frame(&[&a, &b])).partial_uploads, 1);
    assert_eq!(mirror.texture_count(), 1);
    assert!(mirror.decisions().iter().all(|d| d.draw));
}

#[test]
fn two_painted_sizes_in_one_frame_keep_the_sources_own() {
    let (source, writer) = live(4, 4);
    let (_, a) = widget(&source);
    let (_, b) = widget(&source);
    lay_out(&a);
    writer.write_frame(6, 4, &pixels(6, 4, 0), 24).unwrap();
    lay_out(&b);
    // `a` was laid out at 4 × 4, `b` (attached across the resize) at 6 × 4.
    let mut canvas = Canvas::new();
    canvas.draw_live_image(&a, &draw());
    canvas.draw_live_image(&b, &draw());
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&canvas.into_render_frame());
    let d = mirror.decisions();
    assert!(
        !d[0].draw && d[0].deferred,
        "the stale size draws background"
    );
    assert!(d[1].draw, "the source's own size wins");
    assert_eq!(mirror.pixels(source.id()).unwrap().0, 6);

    // And when the source shrank: its own size wins over the larger one.
    let (source, writer) = live(6, 4);
    let (_, a) = widget(&source);
    let (_, b) = widget(&source);
    lay_out(&a);
    writer.write_frame(4, 4, &pixels(4, 4, 0), 16).unwrap();
    lay_out(&b);
    let mut canvas = Canvas::new();
    canvas.draw_live_image(&a, &draw());
    canvas.draw_live_image(&b, &draw());
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&canvas.into_render_frame());
    let d = mirror.decisions();
    assert!(!d[0].draw && d[1].draw, "{d:?}");
    assert_eq!(mirror.pixels(source.id()).unwrap().0, 4);
}

#[test]
fn nothing_to_show_is_observed_and_uploads_nothing() {
    let source = LiveImageSource::new(LivePixelFormat::Rgba8);
    let _writer = source.writer();
    let (_, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    let report = mirror.consume(&frame(&[&consumer]));
    assert_eq!(report, Default::default());
    assert_eq!(mirror.decisions()[0], Default::default());
    assert!(source.is_observed(), "a placeholder counts as observed");
    assert!(
        source.is_displayed(),
        "and as displayed: a producer throttling on it produces the first frame"
    );
    consumer.detach();
    assert!(!source.is_observed());
}

#[test]
fn dropping_the_pass_unobserves_its_consumers() {
    let (source, _writer) = live(4, 4);
    let (_, consumer) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame(&[&consumer]));
    assert!(source.is_observed());
    drop(mirror);
    assert!(!source.is_observed(), "the window closed");
}

// ── C.17: frame plumbing ──

#[test]
fn an_embedded_frame_shifts_its_live_quads_and_merge_shifts_their_commands() {
    let (source, _writer) = live(4, 4);
    let (_, consumer) = widget(&source);
    lay_out(&consumer);
    let mut inner = Canvas::new();
    inner.draw_live_image(&consumer, &draw());
    let inner = inner.into_render_frame();
    let mut outer = Canvas::new();
    outer.draw_live_image(&consumer, &draw());
    outer.draw_render_frame(&inner, Point::new(10.0, 20.0));
    let f = outer.into_render_frame();
    assert_eq!(f.live_images.len(), 2);
    assert_eq!(f.live_images[1].screen, [10.0, 20.0, 64.0, 64.0]);
    assert_eq!(
        f.draw_order,
        vec![DrawCommand::LiveImage(0), DrawCommand::LiveImage(1)]
    );
    let mut merged = RenderFrame::new();
    merged.merge(&f);
    merged.merge(&f);
    assert_eq!(merged.draw_order[3], DrawCommand::LiveImage(3));
    assert_eq!(consumer.stats().attachment.paints, 2, "one per draw call");
}

#[test]
fn a_quad_is_cropped_by_its_texture_coordinates_and_stamped_from_layout() {
    let (source, writer) = live(4, 2);
    let (_, consumer) = widget(&source);
    lay_out(&consumer);
    writer.write_frame(8, 2, &pixels(8, 2, 0), 32).unwrap();
    let mut canvas = Canvas::new();
    let d = LiveImageDraw::new(
        Rect::new(-50.0, 0.0, 200.0, 100.0),
        Rect::new(0.0, 0.0, 100.0, 100.0),
    )
    .filter(ScalingFilter::Nearest);
    canvas.draw_live_image(&consumer, &d);
    let f = canvas.into_render_frame();
    let q = &f.live_images[0];
    assert_eq!(q.screen, [0.0, 0.0, 100.0, 100.0]);
    assert_eq!(q.uv, [[0.25, 0.0], [0.75, 0.0], [0.75, 1.0], [0.25, 1.0]]);
    assert_eq!(q.filter, ScalingFilter::Nearest);
    assert_eq!(
        q.painted,
        Some((4, 2)),
        "the laid-out size, not the newer one"
    );
}

// ── Mip chains, for `ScalingFilter::Trilinear` (spec 6.13) ──

use super::super::internal::{mip_bytes, mip_levels};
use crate::resample::{downsample_half, downsample_half_opaque};

/// Levels 1 and up of `px`, halved level from level as the GPU does.
fn reference_chain(px: &[u8], w: u32, h: u32, opaque: bool) -> Vec<(u32, u32, Vec<u8>)> {
    let mut levels = Vec::new();
    let (mut cw, mut ch, mut cur) = (w, h, px.to_vec());
    for k in 1..mip_levels(w, h) {
        let (nw, nh, next) = if opaque && k == 1 {
            downsample_half_opaque(&cur, cw, ch)
        } else {
            downsample_half(&cur, cw, ch)
        };
        levels.push((nw, nh, next.clone()));
        (cw, ch, cur) = (nw, nh, next);
    }
    levels
}

/// Every level of `source`'s texture is the reference chain of its level 0.
fn assert_chain(mirror: &LiveImageMirror, source: &LiveImageSource, opaque: bool) {
    let (w, h, level0) = mirror.pixels(source.id()).expect("a texture");
    let reference = reference_chain(level0, w, h, opaque);
    assert!(!reference.is_empty());
    for (k, (rw, rh, rpx)) in reference.iter().enumerate() {
        let (lw, lh, lpx) = mirror
            .mip_level(source.id(), k as u32 + 1)
            .expect("the level");
        assert_eq!((lw, lh), (*rw, *rh), "level {}", k + 1);
        assert_eq!(lpx, rpx.as_slice(), "level {}", k + 1);
    }
}

fn trilinear() -> LiveImageDraw {
    draw().filter(ScalingFilter::Trilinear)
}

/// Pixels with transparent texels among them, so a wrong average shows.
fn speckled(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let mut px = pixels(w, h, seed);
    for (i, texel) in px.chunks_mut(4).enumerate() {
        if i % 3 == 0 {
            texel[3] = 0;
        } else if i % 5 == 0 {
            texel[3] = 128;
        }
    }
    px
}

#[test]
fn a_trilinear_quad_stages_a_mipped_texture_and_builds_its_chain() {
    let (source, writer) = live(19, 7);
    writer
        .write_frame(19, 7, &speckled(19, 7, 3), 19 * 4)
        .unwrap();
    let (_, c) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    let report = mirror.consume(&frame_with(&[&c], trilinear()));
    assert_eq!(report.full_uploads, 1);
    assert_eq!(report.mip_rebuilds, vec![None], "the whole chain");
    assert_eq!(mirror.stats().bytes, mip_bytes(19, 7, mip_levels(19, 7)));
    assert_eq!(mirror.stats().mip_updates, 1);
    assert_chain(&mirror, &source, false);
}

#[test]
fn a_rect_rebuilds_only_its_footprint_and_matches_a_whole_rebuild() {
    let (source, writer) = live(37, 23);
    let (_, c) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame_with(&[&c], trilinear()));
    let rect = PixelRect::new(5, 9, 11, 3);
    writer
        .write_rect(rect, &speckled(11, 3, 7), 11 * 4)
        .unwrap();
    let report = mirror.consume(&frame_with(&[&c], trilinear()));
    assert_eq!((report.full_uploads, report.partial_uploads), (0, 1));
    assert_eq!(report.mip_rebuilds, vec![Some(rect)]);
    assert_chain(&mirror, &source, false);
}

#[test]
fn commits_folded_into_one_upload_rebuild_the_chain_once() {
    let (source, writer) = live(16, 16);
    let (_, c) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame_with(&[&c], trilinear()));
    for (i, rect) in [PixelRect::new(0, 0, 2, 2), PixelRect::new(12, 12, 4, 4)]
        .into_iter()
        .enumerate()
    {
        writer
            .write_rect(
                rect,
                &speckled(rect.width, rect.height, i as u8),
                rect.width as usize * 4,
            )
            .unwrap();
    }
    let report = mirror.consume(&frame_with(&[&c], trilinear()));
    assert_eq!(
        report.mip_rebuilds,
        vec![Some(PixelRect::new(0, 0, 16, 16))]
    );
    assert_eq!(mirror.stats().mip_updates, 2);
    assert_chain(&mirror, &source, false);
}

#[test]
fn a_linear_quad_shares_the_mipped_texture_of_a_trilinear_one() {
    let (source, _writer) = live(20, 10);
    let (_, thumb) = widget(&source);
    let (_, main) = widget(&source);
    let mut canvas = Canvas::new();
    lay_out(&thumb);
    lay_out(&main);
    canvas.draw_live_image(&thumb, &trilinear());
    canvas.draw_live_image(&main, &draw());
    let mut mirror = LiveImageMirror::new();
    let report = mirror.consume(&canvas.into_render_frame());
    assert_eq!(report.full_uploads, 1, "one upload for both");
    assert_eq!(mirror.texture_count(), 1);
    assert!(mirror.decisions().iter().all(|d| d.draw));
}

#[test]
fn a_texture_keeps_its_chain_when_trilinear_goes_and_catches_up_when_it_returns() {
    let (source, writer) = live(16, 8);
    let (_, c) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame_with(&[&c], trilinear()));
    let mipped = mirror.stats().bytes;

    // Only `Linear` now: the commits upload into the same texture, and no
    // chain is rebuilt for them.
    for seed in 1..=3 {
        writer
            .write_rect(
                PixelRect::new(seed, 1, 2, 2),
                &speckled(2, 2, seed as u8),
                8,
            )
            .unwrap();
        let report = mirror.consume(&frame(&[&c]));
        assert_eq!((report.full_uploads, report.partial_uploads), (0, 1));
        assert!(report.mip_rebuilds.is_empty());
    }
    assert_eq!(mirror.stats().bytes, mipped, "the levels stay");

    // `Trilinear` again: nothing to upload, the chain catches up with the
    // union of what changed.
    let report = mirror.consume(&frame_with(&[&c], trilinear()));
    assert_eq!((report.full_uploads, report.partial_uploads), (0, 0));
    assert_eq!(report.mip_rebuilds, vec![Some(PixelRect::new(1, 1, 4, 2))]);
    assert_chain(&mirror, &source, false);
}

#[test]
fn trilinear_on_a_texture_without_levels_restages_it_with_them() {
    let (source, _writer) = live(16, 8);
    let (_, c) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame(&[&c]));
    assert_eq!(mirror.stats().bytes, 16 * 8 * 4);
    let report = mirror.consume(&frame_with(&[&c], trilinear()));
    assert_eq!(report.full_uploads, 1, "a new shape: one whole upload");
    assert_eq!(report.mip_rebuilds, vec![None]);
    assert_eq!(mirror.stats().bytes, mip_bytes(16, 8, mip_levels(16, 8)));
    assert_eq!(mirror.texture_count(), 1, "the old one went");
    assert_chain(&mirror, &source, false);
}

#[test]
fn a_new_size_drops_the_levels_no_quad_samples() {
    let (source, writer) = live(16, 8);
    let (_, c) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame_with(&[&c], trilinear()));
    writer.write_frame(8, 8, &pixels(8, 8, 1), 32).unwrap();
    let report = mirror.consume(&frame(&[&c]));
    assert_eq!(report.full_uploads, 1);
    assert_eq!(mirror.stats().bytes, 8 * 8 * 4, "restaged as `Linear` asks");
    assert!(mirror.mip_level(source.id(), 1).is_none());
}

#[test]
fn an_opaque_source_builds_an_opaque_chain() {
    let source = LiveImageSource::new(LivePixelFormat::Rgbx8);
    let writer = source.writer();
    let mut px = pixels(12, 6, 9);
    for texel in px.chunks_mut(4) {
        texel[3] = 0; // what QEMU leaves in the fourth byte
    }
    writer.write_frame(12, 6, &px, 48).unwrap();
    let (_, c) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame_with(&[&c], trilinear()));
    assert_chain(&mirror, &source, true);
    let (_, _, level1) = mirror.mip_level(source.id(), 1).unwrap();
    assert!(
        level1.chunks(4).all(|t| t[3] == 255),
        "every level above 0 opaque"
    );
    assert!(level1.chunks(4).any(|t| t[0] != 0), "and not black");
}

#[test]
fn a_paused_trilinear_source_keeps_its_chain_without_rebuilding() {
    let (source, writer) = live(16, 8);
    let (_, c) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame_with(&[&c], trilinear()));
    writer.write_frame(16, 8, &pixels(16, 8, 4), 64).unwrap();
    let report = mirror.consume(&frame_with(&[&c], trilinear().paused(true)));
    assert_eq!((report.full_uploads, report.paused), (0, 1));
    assert!(report.mip_rebuilds.is_empty());
}

#[test]
fn a_texture_restaged_for_a_large_upload_keeps_its_levels() {
    let (source, writer) = live(32, 32);
    let (_, c) = widget(&source);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame_with(&[&c], trilinear()));
    let mipped = mirror.stats().bytes;
    // Only `Linear` now, and a whole frame larger than one frame's staging:
    // a second texture fills over the next frames, then replaces the first.
    mirror.set_staging_budget(32 * 4 * 8);
    writer.write_frame(32, 32, &pixels(32, 32, 6), 128).unwrap();
    for _ in 0..8 {
        mirror.consume(&frame(&[&c]));
    }
    assert_eq!(
        texture(&mirror, &source),
        pixels(32, 32, 6),
        "the fill completed"
    );
    assert_eq!(mirror.texture_count(), 1);
    assert_eq!(
        mirror.stats().bytes,
        mipped,
        "the replacement has the levels"
    );
    // `Trilinear` returns: the replacement's chain is built, nothing uploads.
    let report = mirror.consume(&frame_with(&[&c], trilinear()));
    assert_eq!((report.full_uploads, report.partial_uploads), (0, 0));
    assert_eq!(report.mip_rebuilds, vec![None]);
    assert_chain(&mirror, &source, false);
}
