// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The diffing writer: what it commits, what it leaves alone, and that the
//! source always ends up holding the frame handed over.

use std::sync::Arc;

use super::super::super::internal::ReadAttempt;
use super::super::super::testing::LiveImageMirror;
use super::super::super::*;
use super::{changed_rects, first_difference, last_difference};
use crate::geometry::Rect;
use crate::wake::{CountingWaker, RedrawWaker};
use crate::{Canvas, RenderFrame};

const W: u32 = 16;
const H: u32 = 12;

/// A `w × h` RGBA frame, pixel `(x, y)` = `[x, y, seed, 255]`.
fn frame(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            px.extend_from_slice(&[x as u8, y as u8, seed, 255]);
        }
    }
    px
}

fn set(px: &mut [u8], w: u32, x: u32, y: u32, value: [u8; 4]) {
    let at = ((y * w + x) * 4) as usize;
    px[at..at + 4].copy_from_slice(&value);
}

/// What the source holds, tightly packed.
fn held(source: &LiveImageSource) -> Vec<u8> {
    let consumer = source.attach(None);
    let ReadAttempt::Locked(read) = consumer.try_read() else {
        panic!("the source is free");
    };
    let (w, h) = read.size().expect("a buffer");
    let mut out = Vec::new();
    for y in 0..h as usize {
        out.extend_from_slice(&read.pixels()[y * read.stride()..][..w as usize * 4]);
    }
    out
}

/// A window showing `source`: its consumer and its waker.
fn window(source: &LiveImageSource) -> (Arc<CountingWaker>, LiveImageConsumer) {
    let waker = Arc::new(CountingWaker::new());
    let consumer = source.attach(Some(waker.clone() as Arc<dyn RedrawWaker>));
    (waker, consumer)
}

fn drawing(consumer: &LiveImageConsumer) -> RenderFrame {
    let _ = consumer.take_geometry();
    consumer.record_layout_meta(consumer.source().meta());
    let rect = Rect::new(0.0, 0.0, 64.0, 64.0);
    let mut canvas = Canvas::new();
    canvas.draw_live_image(consumer, &LiveImageDraw::new(rect, rect));
    canvas.into_render_frame()
}

fn fresh() -> (LiveImageSource, LiveImageDiffWriter) {
    let source = LiveImageSource::new(LivePixelFormat::Rgba8);
    let writer = LiveImageDiffWriter::new(source.writer());
    (source, writer)
}

#[test]
fn the_first_frame_is_written_whole() {
    let (source, mut writer) = fresh();
    let px = frame(W, H, 1);
    let generation = writer.write_frame(W, H, &px, (W * 4) as usize).unwrap();
    assert_eq!(generation, Some(source.generation()));
    assert_eq!(held(&source), px);
    assert_eq!(source.stats().bytes_written, u64::from(W * H * 4));
}

/// An identical frame commits nothing: no generation, no wake, no upload,
/// no byte written.
#[test]
fn an_identical_frame_costs_the_window_nothing() {
    let (source, mut writer) = fresh();
    let (waker, consumer) = window(&source);
    let px = frame(W, H, 1);
    writer.write_frame(W, H, &px, (W * 4) as usize).unwrap();
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&drawing(&consumer));
    let (generation, wakes, stats) = (source.generation(), waker.count(), source.stats());

    for _ in 0..100 {
        assert_eq!(
            writer.write_frame(W, H, &px, (W * 4) as usize).unwrap(),
            None
        );
    }
    assert_eq!(source.generation(), generation);
    assert_eq!(waker.count(), wakes, "no window was woken");
    assert_eq!(source.stats().commits, stats.commits);
    assert_eq!(source.stats().bytes_written, stats.bytes_written);
    let report = mirror.consume(&drawing(&consumer));
    assert_eq!((report.full_uploads, report.partial_uploads), (0, 0));
}

/// A changed pixel commits that pixel, and the window uploads it alone.
#[test]
fn a_changed_pixel_is_all_that_is_committed() {
    let (source, mut writer) = fresh();
    let (_, consumer) = window(&source);
    let mut px = frame(W, H, 1);
    writer.write_frame(W, H, &px, (W * 4) as usize).unwrap();
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&drawing(&consumer));
    let written = source.stats().bytes_written;

    set(&mut px, W, 5, 7, [9, 9, 9, 255]);
    let generation = writer.write_frame(W, H, &px, (W * 4) as usize).unwrap();
    assert_eq!(generation, Some(source.generation()));
    assert_eq!(source.stats().bytes_written - written, 4);
    let report = mirror.consume(&drawing(&consumer));
    assert_eq!((report.full_uploads, report.partial_uploads), (0, 1));
    assert_eq!(report.rects, vec![PixelRect::new(5, 7, 1, 1)]);
    assert_eq!(held(&source), px);
}

#[test]
fn consecutive_changed_rows_are_one_rect_as_wide_as_their_spans() {
    let copy = frame(W, H, 1);
    let mut px = copy.clone();
    // Rows 2 and 3: columns 4 and 9; row 8, column 1.
    set(&mut px, W, 4, 2, [0; 4]);
    set(&mut px, W, 9, 3, [0; 4]);
    set(&mut px, W, 1, 8, [0; 4]);
    let mut rects = Vec::new();
    changed_rects(&mut rects, &copy, &px, (W * 4) as usize, W, H, 4);
    assert_eq!(
        rects,
        vec![PixelRect::new(4, 2, 6, 2), PixelRect::new(1, 8, 1, 1)]
    );
    // A change that touches only the last byte of a pixel still counts the
    // pixel.
    let mut px = copy.clone();
    let at = ((3 * W + 15) * 4 + 3) as usize;
    px[at] = 0;
    changed_rects(&mut rects, &copy, &px, (W * 4) as usize, W, H, 4);
    assert_eq!(rects, vec![PixelRect::new(15, 3, 1, 1)]);
    // And one that touches only its first byte, at either end of the span.
    let mut px = copy.clone();
    px[((5 * W + 2) * 4) as usize] = 0xAA;
    px[((5 * W + 9) * 4) as usize] = 0xAA;
    changed_rects(&mut rects, &copy, &px, (W * 4) as usize, W, H, 4);
    assert_eq!(rects, vec![PixelRect::new(2, 5, 8, 1)]);
}

/// Past sixteen rects, the neighbours whose union adds the fewest pixels
/// merge, so the commit keeps sixteen rects rather than its bounding box.
#[test]
fn more_than_sixteen_bands_merge_their_closest_neighbours() {
    let (w, h) = (8u32, 64u32);
    let copy = frame(w, h, 1);
    let mut px = copy.clone();
    // Seventeen single-row changes: every third row from row 0 to row 45,
    // and row 47. Merging two rows three apart adds two pixels; merging
    // rows 45 and 47 adds one, so that is the pair that merges.
    for i in 0..16 {
        set(&mut px, w, 2, i * 3, [0; 4]);
    }
    set(&mut px, w, 2, 47, [0; 4]);
    let mut rects = Vec::new();
    changed_rects(&mut rects, &copy, &px, (w * 4) as usize, w, h, 4);
    assert_eq!(rects.len(), 16);
    assert_eq!(
        rects[15],
        PixelRect::new(2, 45, 1, 3),
        "rows 45 to 47 merged"
    );
    assert!(rects[..15].iter().all(|r| r.height == 1));

    // Through the writer the commit keeps them, and the window uploads them
    // as rects, not as the frame.
    let source = LiveImageSource::new(LivePixelFormat::Rgba8);
    let (_, consumer) = window(&source);
    let mut writer = LiveImageDiffWriter::new(source.writer());
    writer.write_frame(w, h, &copy, (w * 4) as usize).unwrap();
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&drawing(&consumer));
    writer.write_frame(w, h, &px, (w * 4) as usize).unwrap();
    let report = mirror.consume(&drawing(&consumer));
    assert_eq!((report.full_uploads, report.partial_uploads), (0, 1));
    assert_eq!(report.rects.len(), 16);
    assert_eq!(held(&source), px);
}

#[test]
fn a_padded_stride_is_read_row_by_row() {
    let (source, mut writer) = fresh();
    let stride = (W * 4 + 12) as usize;
    let mut padded = vec![0xEEu8; stride * H as usize];
    let px = frame(W, H, 3);
    for y in 0..H as usize {
        padded[y * stride..][..(W * 4) as usize]
            .copy_from_slice(&px[y * (W * 4) as usize..][..(W * 4) as usize]);
    }
    writer.write_frame(W, H, &padded, stride).unwrap();
    assert_eq!(held(&source), px);
    // The padding never counts as a change.
    padded[(W * 4) as usize] = 0;
    assert_eq!(writer.write_frame(W, H, &padded, stride).unwrap(), None);
    let mut changed = px.clone();
    set(&mut changed, W, 0, 11, [1, 2, 3, 4]);
    padded[11 * stride..][..4].copy_from_slice(&[1, 2, 3, 4]);
    writer.write_frame(W, H, &padded, stride).unwrap();
    assert_eq!(held(&source), changed);
}

#[test]
fn a_new_size_is_written_whole() {
    let (source, mut writer) = fresh();
    let (_, consumer) = window(&source);
    writer
        .write_frame(W, H, &frame(W, H, 1), (W * 4) as usize)
        .unwrap();
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&drawing(&consumer));
    let px = frame(H, W, 1);
    assert!(
        writer
            .write_frame(H, W, &px, (H * 4) as usize)
            .unwrap()
            .is_some()
    );
    assert_eq!(held(&source), px);
    let report = mirror.consume(&drawing(&consumer));
    assert_eq!(report.full_uploads, 1);
}

/// When the source no longer holds this writer's last frame, the next frame
/// goes out whole, identical or not: after another writer's commit, a
/// clear, another writer's abandoned transaction, and a `forget`.
#[test]
fn a_source_changed_behind_its_back_gets_the_whole_frame() {
    let px = frame(W, H, 1);
    let stride = (W * 4) as usize;

    // Another writer committed.
    let (source, mut writer) = fresh();
    writer.write_frame(W, H, &px, stride).unwrap();
    source
        .writer()
        .write_rect(PixelRect::new(0, 0, 2, 2), &[7; 16], 8)
        .unwrap();
    assert!(writer.write_frame(W, H, &px, stride).unwrap().is_some());
    assert_eq!(held(&source), px);

    // A clear: the generation does not move, the status does.
    let (source, mut writer) = fresh();
    writer.write_frame(W, H, &px, stride).unwrap();
    writer.writer().clear().unwrap();
    assert_eq!(source.status(), LiveImageStatus::Waiting);
    assert!(writer.write_frame(W, H, &px, stride).unwrap().is_some());
    assert_eq!(source.status(), LiveImageStatus::Live);
    assert_eq!(held(&source), px);

    // Another writer's abandoned transaction: its pixels are in the buffer,
    // unpublished. The next change, anywhere, goes out whole.
    let (source, mut writer) = fresh();
    writer.write_frame(W, H, &px, stride).unwrap();
    {
        let other = source.writer();
        let mut guard = other.lock().unwrap();
        guard
            .write_rect(PixelRect::new(8, 8, 2, 2), &[1; 16], 8)
            .unwrap();
    }
    let mut changed = px.clone();
    set(&mut changed, W, 0, 0, [5, 5, 5, 255]);
    assert!(
        writer
            .write_frame(W, H, &changed, stride)
            .unwrap()
            .is_some()
    );
    assert_eq!(
        held(&source),
        changed,
        "the abandoned pixels are overwritten"
    );

    // Another writer's transaction wrote through `pixels_mut`, which marks
    // nothing, then panicked: the panic forces the next commit whole.
    let (source, mut writer) = fresh();
    writer.write_frame(W, H, &px, stride).unwrap();
    let other = source.writer();
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut guard = other.lock().unwrap();
        guard.pixels_mut()[..4].copy_from_slice(&[3, 3, 3, 3]);
        panic!("a producer bug");
    }));
    assert!(panicked.is_err());
    let mut changed = px.clone();
    set(&mut changed, W, 9, 9, [6, 6, 6, 255]);
    assert!(
        writer
            .write_frame(W, H, &changed, stride)
            .unwrap()
            .is_some()
    );
    assert_eq!(
        held(&source),
        changed,
        "the unmarked pixels are overwritten"
    );

    // `forget`.
    let (source, mut writer) = fresh();
    let (_, consumer) = window(&source);
    writer.write_frame(W, H, &px, stride).unwrap();
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&drawing(&consumer));
    writer.forget();
    assert!(writer.write_frame(W, H, &px, stride).unwrap().is_some());
    assert_eq!(mirror.consume(&drawing(&consumer)).full_uploads, 1);
}

#[test]
fn a_revoked_writer_is_refused_even_for_an_identical_frame() {
    let (source, mut writer) = fresh();
    let px = frame(W, H, 1);
    writer.write_frame(W, H, &px, (W * 4) as usize).unwrap();
    let _new_session = source.writer_exclusive();
    assert_eq!(
        writer.write_frame(W, H, &px, (W * 4) as usize),
        Err(LiveImageError::Revoked)
    );
}

#[test]
fn a_refused_frame_changes_nothing() {
    let (source, mut writer) = fresh();
    let px = frame(W, H, 1);
    writer.write_frame(W, H, &px, (W * 4) as usize).unwrap();
    let generation = source.generation();
    assert!(matches!(
        writer.write_frame(W, H, &px[..10], (W * 4) as usize),
        Err(LiveImageError::BufferTooSmall { .. })
    ));
    assert!(writer.write_frame(0, H, &px, (W * 4) as usize).is_err());
    assert_eq!(source.generation(), generation);
    // And the writer still knows its last frame.
    assert_eq!(
        writer.write_frame(W, H, &px, (W * 4) as usize).unwrap(),
        None
    );
}

/// Every pixel format compares as bytes: a BGRX frame's fourth byte counts
/// as a change too, as the plain writer would copy it.
#[test]
fn an_alpha_less_format_diffs_every_byte() {
    let source = LiveImageSource::new(LivePixelFormat::Bgrx8);
    let mut writer = LiveImageDiffWriter::new(source.writer());
    let mut px = frame(W, H, 1);
    writer.write_frame(W, H, &px, (W * 4) as usize).unwrap();
    px[3] = 0;
    assert!(
        writer
            .write_frame(W, H, &px, (W * 4) as usize)
            .unwrap()
            .is_some()
    );
    assert_eq!(held(&source), px);
}

/// The chunked scans agree with a byte-by-byte one at every row length
/// around the chunk size and every pair of first and last differing bytes.
#[test]
fn the_chunked_scans_find_the_first_and_last_difference() {
    for len in 1..=200usize {
        let a = vec![7u8; len];
        for first in 0..len {
            for last in [
                first,
                (first + 1).min(len - 1),
                len - 1,
                (first + 63).min(len - 1),
            ] {
                let mut b = a.clone();
                b[first] = 8;
                b[last] = 9;
                assert_eq!(first_difference(&b, &a), first, "len {len}, first {first}");
                assert_eq!(last_difference(&b, &a), last, "len {len}, last {last}");
            }
        }
    }
}
