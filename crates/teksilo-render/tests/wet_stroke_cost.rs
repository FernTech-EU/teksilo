// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a *growing* path costs the mask cache, which is what wet ink is.
//!
//! A stroke being drawn grows by a point per pointer sample and is re-filled
//! every frame. Two costs decide whether that is affordable, and only one of
//! them is the rasterizer:
//!
//! 1. **Building the cache key.** It used to hash every
//!    `PathCommand`, so a *cache hit* — the case where the bitmap is already
//!    resident and nothing needs doing — cost O(n). Over a stroke that is
//!    quadratic before any pixels are touched. `Path::stamp` makes it O(1);
//!    [`a_cache_hit_does_not_scale_with_the_paths_length`] is the gate — the
//!    **only** one, for the reason under "which test gates what" below.
//! 2. **Rasterizing.** Irreducible for a path the cache has not seen, and it
//!    is why re-filling the *whole* growing outline every frame stays
//!    quadratic however cheap the key is
//!    ([`refilling_the_whole_outline_is_still_quadratic`] pins that, so the
//!    docs cannot quietly start claiming otherwise). The pattern that works is
//!    an immutable committed prefix plus a short live tail — every chunk is a
//!    hit, only the tail rasterizes — and [`a_chunked_stroke_stays_flat`] is
//!    what pins that it beats the naive one.
//!
//! Timing tests, so every threshold is a wide ratio between two measurements
//! taken in the same process, never an absolute duration.
//!
//! # Which test gates what
//!
//! The chunked pattern is not O(1) per sample, and this file used to say it
//! was. Per sample it costs `n / CHUNK` cache hits plus one rasterize of a tail
//! of at most `CHUNK + 1` points. The rasterize is a constant floor — the whole
//! per-sample figure at 100 points, where there is barely a chunk to look up, is
//! already 15.7 µs — and the hits are the part that grows with the stroke, which
//! is where (1) earns its keep. But it earns it in that column *modestly*:
//! measured per sample at 2 000 points, **18.3 µs with the stamp against 24.0
//! with the command walk**, beside the 227 µs the naive pattern costs at the
//! same size.
//!
//! A difference of a third is inside a ratio chosen to survive a loaded machine,
//! so [`a_chunked_stroke_stays_flat`] **passes with the stamp removed** and
//! cannot be the stamp's gate. Nor can it be made into one: the comparison that
//! would show the stamp is between two *builds*, and a timing test can only
//! compare two measurements taken in one process. The stamp's gate is
//! [`a_cache_hit_does_not_scale_with_the_paths_length`], which isolates the
//! lookup from the rasterizer and goes from ~1× to 8.4× when the walk is
//! restored.
//!
//! What the chunked test gates is the **pattern** — that freezing the stroke
//! into chunks beats re-filling the whole outline — so it asserts that
//! directly, against a whole-outline measurement taken in the same process,
//! rather than inferring it from a growth ratio alone.
//!
//! Run the numbers behind `docs/ink.md` with
//! `cargo test -p teksilo-render --release --test wet_stroke_cost -- --ignored --nocapture`.

use std::time::{Duration, Instant};
use teksilo_canvas::{FillRule, Path, Point, StrokeStyle};
use teksilo_render::path_atlas::PathAtlas;

/// A closed polygon of `n` points, shaped like a hand-drawn squiggle so the
/// rasterizer has real work rather than one long edge.
fn stroke(n: usize) -> Path {
    let mut p = Path::new();
    p.move_to(Point::new(1.0, 1.0));
    for i in 1..n {
        p.line_to(point_at(i));
    }
    p.close();
    p
}

fn point_at(i: usize) -> Point {
    let t = i as f32 * 0.31;
    Point::new(
        4.0 + t.sin() * 40.0 + i as f32 * 0.002,
        44.0 + t.cos() * 40.0,
    )
}

const BOUNDS: [f32; 4] = [0.0, 0.0, 96.0, 96.0];

fn fill(atlas: &mut PathAtlas, path: &Path) {
    atlas.lookup_or_rasterize(
        path,
        &StrokeStyle::default(),
        FillRule::Winding,
        BOUNDS,
        1.0,
        1.0,
        false,
    );
}

/// How many points a committed chunk holds, as `examples/scene_ink` freezes
/// them.
const CHUNK: usize = 32;

/// Microseconds per pointer sample for the **naive** pattern: re-fill the whole
/// growing outline every frame.
///
/// One `PathAtlas` per call, so the measurement starts cold and the growth it
/// reports is the rasterizer's and not a warmed cache's.
fn whole_outline_per_sample(n: usize) -> f64 {
    let mut atlas = PathAtlas::new(4096, 4096);
    let mut p = Path::new();
    p.move_to(Point::new(1.0, 1.0));
    let t = Instant::now();
    for i in 1..n {
        p.line_to(point_at(i));
        atlas.begin_frame();
        fill(&mut atlas, &p);
    }
    t.elapsed().as_secs_f64() * 1e6 / n as f64
}

/// Microseconds per pointer sample for the pattern the docs recommend: an
/// immutable committed prefix of [`CHUNK`]-point chunks plus a short live tail.
///
/// Every chunk is a cache hit; only the tail rasterizes.
fn chunked_per_sample(n: usize) -> f64 {
    let mut atlas = PathAtlas::new(4096, 4096);
    let mut chunks: Vec<Path> = Vec::new();
    let mut tail = Path::new();
    tail.move_to(Point::new(1.0, 1.0));
    let t = Instant::now();
    for i in 1..n {
        tail.line_to(point_at(i));
        if i % CHUNK == 0 {
            let mut fresh = Path::new();
            fresh.move_to(point_at(i));
            chunks.push(std::mem::replace(&mut tail, fresh));
        }
        atlas.begin_frame();
        for c in &chunks {
            fill(&mut atlas, c);
        }
        fill(&mut atlas, &tail);
    }
    t.elapsed().as_secs_f64() * 1e6 / n as f64
}

/// Median of `rounds` timings of `body`, to take the sting out of a scheduler
/// hiccup on a loaded machine.
fn median(rounds: usize, mut body: impl FnMut()) -> Duration {
    let mut samples: Vec<Duration> = (0..rounds)
        .map(|_| {
            let t = Instant::now();
            body();
            t.elapsed()
        })
        .collect();
    samples.sort_unstable();
    samples[samples.len() / 2]
}

/// The steady state of any incremental wet-ink scheme: the bitmap is resident
/// and the frame's only job is to find it. That lookup must not read the
/// geometry.
#[test]
fn a_cache_hit_does_not_scale_with_the_paths_length() {
    let short = stroke(100);
    let long = stroke(2000);
    let mut atlas = PathAtlas::new(2048, 2048);
    // Prime both, so every timed call below is a hit.
    fill(&mut atlas, &short);
    fill(&mut atlas, &long);

    const REPS: usize = 4000;
    let short_ns = median(7, || {
        for _ in 0..REPS {
            fill(&mut atlas, &short);
        }
    });
    let long_ns = median(7, || {
        for _ in 0..REPS {
            fill(&mut atlas, &long);
        }
    });

    let ratio = long_ns.as_secs_f64() / short_ns.as_secs_f64();
    assert!(
        ratio < 4.0,
        "a cache hit on a 2 000-point path cost {ratio:.1}x one on a 100-point path \
         ({long_ns:?} vs {short_ns:?} for {REPS} lookups). A hit must not read the \
         command list — see `Path::stamp`. Hashing the commands puts it past 8x (measured: 8.6x)."
    );
}

/// The identity the stamp stands in for. If this ever fails the cache is
/// serving one path's bitmap for another's geometry, which is a wrong picture,
/// not a slow one.
#[test]
fn equal_geometry_shares_an_entry_and_different_geometry_does_not() {
    let mut atlas = PathAtlas::new(2048, 2048);
    let a = stroke(64);
    let a_again = stroke(64);
    let mut b = stroke(64);
    b.line_to(Point::new(9.0, 9.0));

    fill(&mut atlas, &a);
    let after_first = atlas.entry_count();
    fill(&mut atlas, &a_again);
    assert_eq!(
        atlas.entry_count(),
        after_first,
        "an identical path must hit the entry the first one made"
    );
    fill(&mut atlas, &b);
    assert_eq!(
        atlas.entry_count(),
        after_first + 1,
        "a path with one more command must not be served the other's mask"
    );
}

/// The honest half of the story: an O(1) key does **not** make the naive
/// "re-fill the whole growing outline every frame" pattern affordable. The
/// rasterizer still sees a longer path each frame.
///
/// Pinned as a test rather than left as prose so `docs/ink.md` cannot
/// drift into recommending it.
#[test]
#[cfg_attr(debug_assertions, ignore = "rasterizer timings need --release")]
fn refilling_the_whole_outline_is_still_quadratic() {
    let small = whole_outline_per_sample(250);
    let large = whole_outline_per_sample(2000);
    assert!(
        large > small * 2.0,
        "re-filling the whole outline was expected to stay superlinear \
         ({large:.1} us/sample at 2 000 points vs {small:.1} at 250). If this \
         has genuinely become flat, `docs/ink.md` needs rewriting rather \
         than this test deleting."
    );
}

/// The pattern the docs recommend: freeze the stroke into immutable chunks as
/// it goes and keep only a short live tail. Every chunk is a cache hit, so the
/// per-sample cost is bounded by the tail rather than by the stroke.
///
/// **This is the gate on the pattern, not on `Path::stamp`** — see the module
/// doc's "which test gates what". The second assertion is the one that says so
/// out loud: it compares the two patterns at the same length, in the same
/// process, so abandoning chunking reddens it whatever the machine's mood.
#[test]
#[cfg_attr(debug_assertions, ignore = "rasterizer timings need --release")]
fn a_chunked_stroke_stays_flat() {
    let small = chunked_per_sample(250);
    let large = chunked_per_sample(2000);
    assert!(
        large < small * 3.0,
        "a chunked stroke should stay near flat: {large:.1} us/sample at 2 000 \
         points against {small:.1} at 250. Anything worse means a chunk stopped \
         hitting the cache."
    );

    // …and flat is worth having, which is a claim about the alternative and so
    // has to be measured against it rather than asserted about one curve.
    let naive = whole_outline_per_sample(2000);
    assert!(
        large * 4.0 < naive,
        "at 2 000 points the chunked pattern cost {large:.1} us/sample against \
         the whole-outline refill's {naive:.1} — under the 4x the docs claim \
         for it. Either chunking stopped working or `docs/ink.md` is \
         recommending the wrong thing."
    );
}

/// Prints the table quoted in `docs/ink.md`. Not a gate.
#[test]
#[ignore = "measurement, printed for the docs"]
fn measure() {
    println!("n        cache-hit    whole-outline/sample   chunked/sample");
    for n in [100usize, 500, 2000] {
        let path = stroke(n);
        let mut atlas = PathAtlas::new(2048, 2048);
        fill(&mut atlas, &path);
        const REPS: usize = 4000;
        let hit = median(5, || {
            for _ in 0..REPS {
                fill(&mut atlas, &path);
            }
        });
        let hit_ns = hit.as_secs_f64() * 1e9 / REPS as f64;

        // The same two functions the gates measure, so the table and the
        // assertions can never be reporting different things.
        let whole_us = whole_outline_per_sample(n);
        let chunk_us = chunked_per_sample(n);

        println!("{n:<8} {hit_ns:>8.1} ns  {whole_us:>16.1} us  {chunk_us:>13.1} us");
    }
}
