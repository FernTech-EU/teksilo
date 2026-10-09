// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! How long the live pass takes, as histograms: what the frame-time and
//! latency measurements of live pictures read. Built in debug builds and
//! with the `live-image-timings` feature; a release build without it keeps
//! no samples.
//!
//! Three quantities, each over its last [`SAMPLES`] samples, in whole
//! microseconds:
//!
//! - `prepare`: one render's live pass, mip chains included, for a frame
//!   that draws a live picture;
//! - `lock_hold`: each hold of a source's lock by the renderer, which is how
//!   long a producer can be kept waiting;
//! - `commit_to_upload`: for each completed upload, from the commit of the
//!   generation it uploaded to the end of its writes.

use std::time::Duration;

use teksilo_canvas::live_image::internal::PassTimings;

/// How many of the latest samples a histogram covers.
pub const SAMPLES: usize = 1024;

/// The 50th, 90th and 99th percentiles and the largest of a histogram's
/// samples, in whole microseconds, by nearest rank; all 0 with no sample.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Percentiles {
    pub p50: u32,
    pub p90: u32,
    pub p99: u32,
    pub max: u32,
    /// How many samples they are over: at most [`SAMPLES`].
    pub samples: u32,
}

/// What [`Renderer::live_texture_timings`](crate::Renderer::live_texture_timings)
/// reports.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct LiveImageTimings {
    /// One render's live pass, mip chains included, for a frame that draws
    /// a live picture.
    pub prepare: Percentiles,
    /// Each hold of a source's lock by the renderer.
    pub lock_hold: Percentiles,
    /// Each completed upload, from its generation's commit to the end of
    /// its writes.
    pub commit_to_upload: Percentiles,
}

/// The latest [`SAMPLES`] durations, in microseconds.
#[derive(Debug)]
struct Ring {
    samples: Vec<u32>,
    next: usize,
}

impl Ring {
    fn new() -> Self {
        Self {
            samples: Vec::with_capacity(SAMPLES),
            next: 0,
        }
    }

    fn push(&mut self, duration: Duration) {
        let micros = u32::try_from(duration.as_micros()).unwrap_or(u32::MAX);
        if self.samples.len() < SAMPLES {
            self.samples.push(micros);
        } else {
            self.samples[self.next] = micros;
        }
        self.next = (self.next + 1) % SAMPLES;
    }

    fn percentiles(&self) -> Percentiles {
        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        let Some(&max) = sorted.last() else {
            return Percentiles::default();
        };
        // Nearest rank: the smallest sample with at least `p` of them at or
        // below it.
        let rank = |p: usize| sorted[(p * sorted.len()).div_ceil(100) - 1];
        Percentiles {
            p50: rank(50),
            p90: rank(90),
            p99: rank(99),
            max,
            samples: sorted.len() as u32,
        }
    }
}

/// The three histograms a renderer keeps.
#[derive(Debug)]
pub(crate) struct LiveTimingRings {
    prepare: Ring,
    lock_hold: Ring,
    commit_to_upload: Ring,
}

impl LiveTimingRings {
    pub(crate) fn new() -> Self {
        Self {
            prepare: Ring::new(),
            lock_hold: Ring::new(),
            commit_to_upload: Ring::new(),
        }
    }

    /// One render's samples: its live pass, when it drew a live picture, and
    /// what the pass timed under the sources' locks.
    pub(crate) fn record(&mut self, prepare: Option<Duration>, pass: &PassTimings) {
        if let Some(prepare) = prepare {
            self.prepare.push(prepare);
        }
        for &hold in &pass.lock_holds {
            self.lock_hold.push(hold);
        }
        for &delay in &pass.commit_to_upload {
            self.commit_to_upload.push(delay);
        }
    }

    pub(crate) fn timings(&self) -> LiveImageTimings {
        LiveImageTimings {
            prepare: self.prepare.percentiles(),
            lock_hold: self.lock_hold.percentiles(),
            commit_to_upload: self.commit_to_upload.percentiles(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring_of(micros: impl IntoIterator<Item = u64>) -> Ring {
        let mut ring = Ring::new();
        for us in micros {
            ring.push(Duration::from_micros(us));
        }
        ring
    }

    #[test]
    fn percentiles_are_by_nearest_rank() {
        let p = ring_of(1..=100).percentiles();
        assert_eq!(
            (p.p50, p.p90, p.p99, p.max, p.samples),
            (50, 90, 99, 100, 100)
        );
        // Ten samples: the 99th percentile is the largest.
        let p = ring_of([7, 1, 3, 9, 5, 2, 8, 4, 10, 6]).percentiles();
        assert_eq!((p.p50, p.p90, p.p99, p.max), (5, 9, 10, 10));
        let p = ring_of([42]).percentiles();
        assert_eq!((p.p50, p.p90, p.p99, p.max, p.samples), (42, 42, 42, 42, 1));
        assert_eq!(Ring::new().percentiles(), Percentiles::default());
    }

    #[test]
    fn a_histogram_covers_its_latest_samples_only() {
        // 1024 samples of 1000 µs, then 1024 of 1 µs: the first are gone.
        let ring =
            ring_of(std::iter::repeat_n(1000, SAMPLES).chain(std::iter::repeat_n(1, SAMPLES)));
        let p = ring.percentiles();
        assert_eq!((p.max, p.samples), (1, SAMPLES as u32));
        // Half overwritten: the old ones still count.
        let ring =
            ring_of(std::iter::repeat_n(1000, SAMPLES).chain(std::iter::repeat_n(1, SAMPLES / 2)));
        let p = ring.percentiles();
        assert_eq!((p.p50, p.max, p.samples), (1, 1000, SAMPLES as u32));
    }

    #[test]
    fn a_duration_past_u32_microseconds_saturates() {
        let mut ring = Ring::new();
        ring.push(Duration::from_secs(10_000));
        ring.push(Duration::from_nanos(999));
        let p = ring.percentiles();
        assert_eq!((p.p50, p.max), (0, u32::MAX));
    }

    #[test]
    fn a_render_records_its_prepare_and_what_its_pass_timed() {
        let mut rings = LiveTimingRings::new();
        let pass = PassTimings {
            lock_holds: vec![Duration::from_micros(30), Duration::from_micros(10)],
            commit_to_upload: vec![Duration::from_micros(500)],
        };
        rings.record(Some(Duration::from_micros(80)), &pass);
        rings.record(None, &PassTimings::default());
        let t = rings.timings();
        assert_eq!((t.prepare.samples, t.prepare.max), (1, 80));
        assert_eq!(
            (t.lock_hold.samples, t.lock_hold.p50, t.lock_hold.max),
            (2, 10, 30)
        );
        assert_eq!(
            (t.commit_to_upload.samples, t.commit_to_upload.max),
            (1, 500)
        );
    }
}
