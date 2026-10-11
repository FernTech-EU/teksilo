// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared column geometry: how many columns fit a given width, and where
//! each one starts.
//!
//! Two unrelated widgets need the same horizontal math and must not drift
//! apart:
//!
//! * [`GridView`](crate::grid_view::GridView) — every grid strategy (uniform,
//!   variable-row, waterfall) derives its column count and per-column
//!   `(x, width)` this way; only the *vertical* layout differs.
//! * [`ColumnFlow`](crate::primitives::ColumnFlow) — derives its column count
//!   the same way, then flows children into those columns column-major.
//!
//! The column-count rule for [`WidthPolicy::Adaptive`] is the CSS
//! `repeat(auto-fill, minmax(w, 1fr))` formula — `floor((avail + gap) / (w + gap))`,
//! floored at 1 — which is also what SwiftUI's `GridItem(.adaptive(minimum:))`
//! and Jetpack Compose's `GridCells.Adaptive(minSize)` compute. Leftover width
//! is shared evenly across the resulting columns rather than trailing after the
//! last one (again matching Compose).
//!
//! This module is deliberately policy-free: it knows nothing about tiles, rows,
//! or data sources. `GridView` maps its own `GridSizing` onto [`WidthPolicy`]
//! at the call site.

use teksilo_canvas::EdgeInsets;
use teksilo_tokens::HAlignment;

/// How a column's width is determined.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum WidthPolicy {
    /// Fixed column width; columns are not stretched, and leftover width
    /// trails after the last column.
    Fixed(f32),
    /// Explicit column count; columns stretch to an equal share.
    Count(usize),
    /// Minimum column width; columns stretch to fill, clamped to `max` when
    /// set. The CSS `auto-fill` / SwiftUI `.adaptive` / Compose
    /// `GridCells.Adaptive` policy.
    Adaptive { min: f32, max: Option<f32> },
}

/// Horizontal layout: column count + per-column geometry.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ColumnGeometry {
    policy: WidthPolicy,
    col_gap: f32,
    inset: EdgeInsets,
    max_columns: Option<usize>,
    min_columns: Option<usize>,
    align: HAlignment,
    rtl: bool,
}

impl ColumnGeometry {
    /// Build from a resolved [`WidthPolicy`].
    ///
    /// A negative `col_gap` is clamped to zero, so a caller cannot make the
    /// column-count division blow up.
    pub(crate) fn from_policy(policy: WidthPolicy, col_gap: f32, inset: EdgeInsets) -> Self {
        Self {
            policy,
            col_gap: col_gap.max(0.0),
            inset,
            max_columns: None,
            min_columns: None,
            align: HAlignment::Leading,
            rtl: false,
        }
    }

    /// Cap the column count, however wide the viewport gets. Columns then
    /// stretch to share the width between the capped count.
    ///
    /// This is CSS's `columns: <width> <count>` pair — when `column-width` and
    /// `column-count` are both given, the count is a *maximum*. The cap must
    /// live here rather than at the call site: [`column_width`] and
    /// [`used_width`] divide by the count, so a cap applied outside would size
    /// columns for one count while the caller placed another.
    ///
    /// [`column_width`]: Self::column_width
    /// [`used_width`]: Self::used_width
    pub(crate) fn with_max_columns(mut self, max: Option<usize>) -> Self {
        self.max_columns = max.map(|m| m.max(1));
        self
    }

    /// Floor the column count, however narrow the viewport gets.
    ///
    /// Under [`WidthPolicy::Fixed`] a floor the width cannot afford makes the
    /// columns **shrink** to share the width between the floored count — a
    /// fixed-width column never overflows the viewport once a floor is set.
    /// Where a floor and a [`with_max_columns`](Self::with_max_columns) cap
    /// disagree, the cap wins. Without a floor, [`WidthPolicy::Fixed`] keeps
    /// its width whatever the viewport.
    pub(crate) fn with_min_columns(mut self, min: Option<usize>) -> Self {
        self.min_columns = min.map(|m| m.max(1));
        self
    }

    /// Where the block of columns sits when it is narrower than the content
    /// width (a [`WidthPolicy::Fixed`] width, an `Adaptive` `max`, a column
    /// cap). `rtl` swaps leading and trailing, exactly as
    /// [`HAlignment::resolve`] does. The default, leading in a left-to-right
    /// layout, puts the block against the leading inset.
    pub(crate) fn with_alignment(mut self, align: HAlignment) -> Self {
        self.align = align;
        self
    }

    /// The layout direction [`with_alignment`](Self::with_alignment) resolves
    /// leading and trailing against.
    pub(crate) fn with_rtl(mut self, rtl: bool) -> Self {
        self.rtl = rtl;
        self
    }

    /// Usable content width inside the leading/trailing insets.
    pub(crate) fn available_width(&self, viewport_width: f32) -> f32 {
        (viewport_width - self.inset.horizontal()).max(0.0)
    }

    /// How many columns fit `viewport_width`. Always at least 1 — a viewport
    /// narrower than one column still shows a single (squeezed) column rather
    /// than nothing.
    pub(crate) fn column_count(&self, viewport_width: f32) -> usize {
        let fit = match self.policy {
            WidthPolicy::Count(n) => n.max(1),
            WidthPolicy::Fixed(w) | WidthPolicy::Adaptive { min: w, .. } => {
                let avail = self.available_width(viewport_width);
                if w <= 0.0 {
                    return self.bounded(1);
                }
                // `floor((avail + gap) / (w + gap))` — the CSS auto-fill count.
                // Adding one gap to the numerator accounts for there being one
                // fewer gap than columns. A tiny epsilon on the ratio before
                // flooring absorbs float noise from upstream layout math
                // (e.g. a `viewport_width` that should exactly fit N columns
                // landing a hair under N*(w+gap)) so an exact fit doesn't
                // intermittently lose a column; it's far smaller than one
                // real pixel, so a genuine under-fit still floors down.
                const EPSILON: f32 = 1e-4;
                let n = ((avail + self.col_gap) / (w + self.col_gap) + EPSILON).floor() as i64;
                n.max(1) as usize
            }
        };
        self.bounded(fit)
    }

    /// `fit` raised to the floor, then cut to the cap, never below 1.
    fn bounded(&self, fit: usize) -> usize {
        let fit = fit.max(self.min_columns.unwrap_or(1));
        match self.max_columns {
            Some(max) => fit.min(max).max(1),
            None => fit.max(1),
        }
    }

    /// The width every column takes at `viewport_width`. Columns stretch to
    /// an even share of the leftover under [`WidthPolicy::Count`] and
    /// [`WidthPolicy::Adaptive`] (clamped by the latter's `max`);
    /// [`WidthPolicy::Fixed`] never stretches, but it does shrink when a
    /// [`with_min_columns`](Self::with_min_columns) floor forces more columns
    /// than fit.
    pub(crate) fn column_width(&self, viewport_width: f32) -> f32 {
        let cols = self.column_count(viewport_width).max(1);
        let avail = self.available_width(viewport_width);
        let stretched = (avail - self.col_gap * (cols as f32 - 1.0)) / cols as f32;
        match self.policy {
            // Never wider than the share, once a floor on the count may have
            // forced more columns than the width affords.
            WidthPolicy::Fixed(w) if self.min_columns.is_some() => w.min(stretched),
            WidthPolicy::Fixed(w) => w,
            WidthPolicy::Count(_) => stretched,
            WidthPolicy::Adaptive { max, .. } => match max {
                Some(mx) => stretched.min(mx),
                None => stretched,
            },
        }
        .max(0.0)
    }

    /// The `(x, width)` of column `col` at `viewport_width`, in logical pixels
    /// relative to the container's leading edge. Includes the offset
    /// [`with_alignment`](Self::with_alignment) puts on the whole block.
    pub(crate) fn column_x(&self, col: usize, viewport_width: f32) -> (f32, f32) {
        let col_w = self.column_width(viewport_width);
        let x = self.inset.leading
            + self.block_offset(viewport_width)
            + col as f32 * (col_w + self.col_gap);
        (x, col_w)
    }

    /// How far the block of columns sits from the leading inset: zero when it
    /// fills the content width, otherwise the share of the leftover the
    /// alignment asks for. Never negative, so a block wider than the content
    /// (a single fixed column in a narrow viewport) stays at the inset.
    pub(crate) fn block_offset(&self, viewport_width: f32) -> f32 {
        let avail = self.available_width(viewport_width);
        self.align
            .resolve(self.used_width(viewport_width), avail, self.rtl)
            .max(0.0)
    }

    /// Total width actually consumed by the columns plus their gaps, i.e. the
    /// used extent inside the insets. Equals [`available_width`] unless a
    /// [`WidthPolicy::Adaptive`] `max` (or a [`WidthPolicy::Fixed`] width)
    /// clamps the columns narrower than their even share, in which case the
    /// difference is the leftover a caller may distribute.
    ///
    /// [`available_width`]: Self::available_width
    pub(crate) fn used_width(&self, viewport_width: f32) -> f32 {
        let cols = self.column_count(viewport_width).max(1);
        let col_w = self.column_width(viewport_width);
        col_w * cols as f32 + self.col_gap * (cols as f32 - 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adaptive(min: f32, max: Option<f32>, gap: f32) -> ColumnGeometry {
        ColumnGeometry::from_policy(WidthPolicy::Adaptive { min, max }, gap, EdgeInsets::ZERO)
    }

    #[test]
    fn adaptive_count_is_css_auto_fill_formula() {
        // 1000 wide, min 240, gap 0 -> floor(1000 / 240) = 4
        assert_eq!(adaptive(240.0, None, 0.0).column_count(1000.0), 4);
        // 1000 wide, min 240, gap 16 -> floor(1016 / 256) = 3
        assert_eq!(adaptive(240.0, None, 16.0).column_count(1000.0), 3);
    }

    #[test]
    fn adaptive_count_floors_at_one() {
        assert_eq!(adaptive(240.0, None, 0.0).column_count(10.0), 1);
        assert_eq!(adaptive(240.0, None, 0.0).column_count(0.0), 1);
    }

    #[test]
    fn zero_min_width_does_not_divide_by_zero() {
        assert_eq!(adaptive(0.0, None, 0.0).column_count(1000.0), 1);
    }

    #[test]
    fn exact_fit_is_stable_under_float_rounding() {
        // `avail` is supposed to fit exactly 4 columns of 240 (no gap), but
        // land a hair under the exact boundary — the kind of sub-pixel
        // float noise `(avail + gap) / (w + gap)` can pick up from upstream
        // layout math (e.g. 3.999999... instead of 4.0). Without the
        // stabilizing epsilon this floors to 3, silently dropping a column
        // that should fit.
        let g = adaptive(240.0, None, 0.0);
        let avail = 4.0 * 240.0 - 0.0001;
        assert_eq!(
            g.column_count(avail),
            4,
            "an exact (up to float noise) fit must not drop a column"
        );
    }

    #[test]
    fn one_pixel_under_still_drops_a_column() {
        // A genuine, non-noise shortfall — the epsilon must not swallow it.
        let g = adaptive(240.0, None, 0.0);
        let avail = 4.0 * 240.0 - 1.0;
        assert_eq!(g.column_count(avail), 3);
    }

    #[test]
    fn negative_gap_is_clamped() {
        // A negative gap would otherwise inflate the count.
        assert_eq!(adaptive(240.0, None, -50.0).column_count(1000.0), 4);
    }

    #[test]
    fn adaptive_columns_stretch_to_fill() {
        // 3 columns in 1000 with gap 20 -> (1000 - 40) / 3 = 320 each.
        let g = adaptive(300.0, None, 20.0);
        assert_eq!(g.column_count(1000.0), 3);
        let (x0, w0) = g.column_x(0, 1000.0);
        let (x1, _) = g.column_x(1, 1000.0);
        assert!((w0 - 320.0).abs() < 0.01);
        assert!((x0 - 0.0).abs() < 0.01);
        assert!((x1 - 340.0).abs() < 0.01);
    }

    #[test]
    fn adaptive_max_clamps_column_width() {
        // Would stretch to 500 each; max pins them at 300.
        let g = adaptive(240.0, Some(300.0), 0.0);
        assert_eq!(g.column_count(1000.0), 4);
        let (_, w) = g.column_x(0, 1000.0);
        assert!((w - 250.0).abs() < 0.01, "stretched share 250 is under max");

        let g = adaptive(400.0, Some(300.0), 0.0);
        assert_eq!(g.column_count(1000.0), 2);
        let (_, w) = g.column_x(0, 1000.0);
        assert!((w - 300.0).abs() < 0.01, "share 500 clamped to max 300");
    }

    #[test]
    fn used_width_reports_leftover_when_max_clamps() {
        let g = adaptive(400.0, Some(300.0), 0.0);
        // 2 columns of 300 = 600 used out of 1000 available.
        assert!((g.used_width(1000.0) - 600.0).abs() < 0.01);
        assert!((g.available_width(1000.0) - 1000.0).abs() < 0.01);
    }

    #[test]
    fn used_width_equals_available_when_unclamped() {
        let g = adaptive(300.0, None, 20.0);
        assert!((g.used_width(1000.0) - 1000.0).abs() < 0.01);
    }

    #[test]
    fn insets_reduce_available_width_and_offset_columns() {
        let g = ColumnGeometry::from_policy(
            WidthPolicy::Adaptive {
                min: 200.0,
                max: None,
            },
            0.0,
            EdgeInsets {
                leading: 10.0,
                trailing: 10.0,
                top: 0.0,
                bottom: 0.0,
            },
        );
        // 1000 - 20 = 980 available -> floor(980/200) = 4
        assert_eq!(g.column_count(1000.0), 4);
        let (x0, w0) = g.column_x(0, 1000.0);
        assert!((x0 - 10.0).abs() < 0.01);
        assert!((w0 - 245.0).abs() < 0.01);
    }

    #[test]
    fn max_columns_caps_the_count_and_widens_the_columns() {
        // The regression: capping the count outside the solver left
        // `column_width` dividing by the uncapped count, so columns were sized
        // for 5 columns while only 2 were placed.
        let g = adaptive(200.0, None, 0.0).with_max_columns(Some(2));
        assert_eq!(g.column_count(1000.0), 2, "5 would fit; cap wins");
        assert!(
            (g.column_width(1000.0) - 500.0).abs() < 0.01,
            "columns share the width between the CAPPED count, got {}",
            g.column_width(1000.0)
        );
        assert!((g.used_width(1000.0) - 1000.0).abs() < 0.01);
    }

    #[test]
    fn max_columns_does_not_raise_the_count() {
        let g = adaptive(400.0, None, 0.0).with_max_columns(Some(5));
        assert_eq!(g.column_count(1000.0), 2, "only 2 fit; cap is a ceiling");
    }

    #[test]
    fn max_columns_floors_at_one() {
        let g = adaptive(200.0, None, 0.0).with_max_columns(Some(0));
        assert_eq!(g.column_count(1000.0), 1);
    }

    #[test]
    fn max_columns_none_is_uncapped() {
        let g = adaptive(200.0, None, 0.0).with_max_columns(None);
        assert_eq!(g.column_count(1000.0), 5);
    }

    #[test]
    fn fixed_count_ignores_width() {
        let g = ColumnGeometry::from_policy(WidthPolicy::Count(3), 0.0, EdgeInsets::ZERO);
        assert_eq!(g.column_count(1000.0), 3);
        assert_eq!(g.column_count(50.0), 3);
        let (_, w) = g.column_x(0, 900.0);
        assert!((w - 300.0).abs() < 0.01);
    }

    #[test]
    fn fixed_count_floors_at_one() {
        let g = ColumnGeometry::from_policy(WidthPolicy::Count(0), 0.0, EdgeInsets::ZERO);
        assert_eq!(g.column_count(1000.0), 1);
    }

    fn fixed(w: f32, gap: f32) -> ColumnGeometry {
        ColumnGeometry::from_policy(WidthPolicy::Fixed(w), gap, EdgeInsets::ZERO)
    }

    #[test]
    fn min_columns_raises_the_count_and_shrinks_fixed_columns_to_fit() {
        let g = fixed(100.0, 10.0).with_min_columns(Some(3));
        // 150 fits one column; the floor forces three that share 150 - 20.
        assert_eq!(g.column_count(150.0), 3);
        assert!((g.column_width(150.0) - 130.0 / 3.0).abs() < 0.01);
        assert!((g.used_width(150.0) - 150.0).abs() < 0.01, "no overflow");
        // Room for the floor: the width is the policy's again.
        assert!((g.column_width(1000.0) - 100.0).abs() < 0.01);
    }

    #[test]
    fn without_a_floor_a_fixed_column_keeps_its_width_in_a_narrow_viewport() {
        assert!((fixed(100.0, 0.0).column_width(40.0) - 100.0).abs() < 0.01);
    }

    #[test]
    fn the_cap_beats_the_floor() {
        let g = fixed(100.0, 0.0)
            .with_min_columns(Some(5))
            .with_max_columns(Some(2));
        assert_eq!(g.column_count(1000.0), 2);
    }

    #[test]
    fn alignment_offsets_the_block_by_its_share_of_the_leftover() {
        // 4 columns of 100 in 450: 50 left over.
        let at = |a, rtl| {
            fixed(100.0, 0.0)
                .with_alignment(a)
                .with_rtl(rtl)
                .column_x(0, 450.0)
                .0
        };
        assert!((at(HAlignment::Leading, false) - 0.0).abs() < 0.01);
        assert!((at(HAlignment::Center, false) - 25.0).abs() < 0.01);
        assert!((at(HAlignment::Trailing, false) - 50.0).abs() < 0.01);
        assert!((at(HAlignment::Leading, true) - 50.0).abs() < 0.01);
        assert!((at(HAlignment::Trailing, true) - 0.0).abs() < 0.01);
    }

    #[test]
    fn alignment_is_measured_inside_the_insets_and_never_goes_negative() {
        let inset = EdgeInsets {
            leading: 10.0,
            trailing: 20.0,
            top: 0.0,
            bottom: 0.0,
        };
        let g = ColumnGeometry::from_policy(WidthPolicy::Fixed(100.0), 0.0, inset)
            .with_alignment(HAlignment::Center);
        // 450 - 30 = 420 available, 4 columns = 400 used: offset 10 + 10.
        assert!((g.column_x(0, 450.0).0 - 20.0).abs() < 0.01);
        // One 100 column in 60: wider than the content, stays at the inset.
        assert!((g.column_x(0, 90.0).0 - 10.0).abs() < 0.01);
    }

    #[test]
    fn fixed_width_does_not_stretch() {
        let g = ColumnGeometry::from_policy(WidthPolicy::Fixed(150.0), 0.0, EdgeInsets::ZERO);
        assert_eq!(g.column_count(1000.0), 6);
        let (_, w) = g.column_x(0, 1000.0);
        assert!((w - 150.0).abs() < 0.01, "fixed width never stretches");
    }
}

/// Property-based tests for [`ColumnGeometry`] under [`WidthPolicy::Adaptive`]
/// — the policy `ColumnFlow` and `GridView`'s adaptive strategy both use.
///
/// Both `ColumnGeometry` and `WidthPolicy` are `pub(crate)` (see
/// `pub(crate) mod column_geometry;` in `common.rs`), so this suite lives
/// inline rather than in `tests/`, which cannot reach a `pub(crate)` item.
///
/// The module doc states the column-count rule is the CSS `auto-fill`
/// formula `floor((avail + gap) / (min + gap))`, floored at 1, and that
/// `with_max_columns` is a cap the formula must never exceed. Those, plus
/// determinism-adjacent facts like "widening never loses a column", "a
/// negative gap is clamped, not merely tolerated", and "`used_width` /
/// `column_width` respect the configured `max_column_width` clamp", are the
/// properties below.
///
/// **The floor formula includes an epsilon.** `0fb72869` ("column epsilon")
/// added a `+ 1e-4` stabilizer inside `column_count` *after* this suite was
/// first drafted, so its oracle must replicate that epsilon rather than
/// assert a bare floor — see the comment on property 3 for why a bare-floor
/// oracle would be a real (not hypothetical) source of flaky failures here,
/// not merely a stricter assertion.
///
/// `cargo-fuzz` needs nightly + libfuzzer-sys, which isn't assumed here;
/// proptest with 256 cases per property (override with `PROPTEST_CASES=N`)
/// covers the width/min/gap/cap combinations a hand-written example table
/// would miss, with shrinking to a minimal counterexample on failure.
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    // A collapsed (0) and a hairline (1) viewport are edge cases the
    // column-count formula must not divide-by-zero or misbehave on.
    fn arb_width() -> impl Strategy<Value = f32> {
        prop_oneof![Just(0.0_f32), Just(1.0_f32), 1.0f32..4000.0_f32,]
    }

    // `min <= 0.0` is a documented special case (pins to a single column);
    // bias toward hitting it alongside ordinary card-ish widths.
    fn arb_min_width() -> impl Strategy<Value = f32> {
        prop_oneof![Just(0.0_f32), 1.0f32..600.0_f32,]
    }

    // Zero, negative (clamped), and a huge gap are the documented edge
    // cases; a mid-range gap is the common case.
    fn arb_gap() -> impl Strategy<Value = f32> {
        prop_oneof![
            Just(0.0_f32),
            Just(-25.0_f32),
            0.0f32..80.0_f32,
            Just(10_000.0_f32),
        ]
    }

    fn arb_max_columns() -> impl Strategy<Value = Option<usize>> {
        prop_oneof![Just(None), (1usize..6usize).prop_map(Some),]
    }

    // `max_column_width` (the per-column stretch clamp) is a completely
    // different knob from `max_columns` (the count cap) — `None` alongside
    // a mix of concrete clamps so properties exercise both the identity
    // path (no clamp: `column_width` == the even stretched share) and the
    // clamped path (a genuine `used_width < available_width` gap opens up).
    fn arb_max_column_width() -> impl Strategy<Value = Option<f32>> {
        prop_oneof![Just(None), (1.0f32..600.0_f32).prop_map(Some),]
    }

    fn adaptive(min: f32, gap: f32, max_columns: Option<usize>) -> ColumnGeometry {
        ColumnGeometry::from_policy(
            WidthPolicy::Adaptive { min, max: None },
            gap,
            EdgeInsets::ZERO,
        )
        .with_max_columns(max_columns)
    }

    // ── 1. column count never exceeds the configured cap ──
    proptest! {
        #[test]
        fn column_count_never_exceeds_max_columns(
            width in arb_width(), min in arb_min_width(), gap in arb_gap(), max in 1usize..6usize,
        ) {
            let g = adaptive(min, gap, Some(max));
            let count = g.column_count(width);
            prop_assert!(
                count <= max,
                "column_count {} exceeds max_columns {} at width {}", count, max, width
            );
        }
    }

    // ── 2. column count is never zero ──
    proptest! {
        #[test]
        fn column_count_is_always_at_least_one(
            width in arb_width(), min in arb_min_width(), gap in arb_gap(), max in arb_max_columns(),
        ) {
            let g = adaptive(min, gap, max);
            prop_assert!(
                g.column_count(width) >= 1,
                "column_count returned 0 at width {} min {} gap {}", width, min, gap
            );
        }
    }

    // ── 3. adaptive count matches the documented epsilon-stabilized floor formula ──
    proptest! {
        #[test]
        fn adaptive_count_matches_the_documented_floor_formula(
            width in 1.0f32..4000.0_f32, min in 1.0f32..600.0_f32, gap in 0.0f32..80.0_f32,
        ) {
            // `column_count` computes `floor((avail + gap) / (min + gap) + EPSILON)`,
            // NOT a bare floor (see the `EPSILON` comment on `column_count`,
            // added by 0fb72869 "column epsilon" specifically so an
            // exact-fit width landing a hair under the true boundary from
            // upstream float noise doesn't silently drop a column). A
            // bare-floor oracle disagrees with the real implementation
            // whenever the ratio falls inside that ~1e-4-wide boundary band
            // — which random continuous `width`/`min`/`gap` draws hit often
            // enough over hundreds of cases to be a real (not hypothetical)
            // source of spurious failures — so the oracle must replicate the
            // same epsilon to test the actual contract rather than a
            // slightly different one.
            const EPSILON: f32 = 1e-4;
            let g = adaptive(min, gap, None);
            let expected = (((width + gap) / (min + gap) + EPSILON).floor() as i64).max(1) as usize;
            prop_assert_eq!(
                g.column_count(width), expected,
                "formula mismatch at width {} min {} gap {}", width, min, gap
            );
        }
    }

    // ── 4. widening the viewport never loses a column ──
    proptest! {
        #[test]
        fn column_count_is_monotone_nondecreasing_in_width(
            min in arb_min_width(), gap in arb_gap(), max in arb_max_columns(),
            narrow in 0.0f32..2000.0_f32, extra in 0.0f32..2000.0_f32,
        ) {
            let g = adaptive(min, gap, max);
            let wide = narrow + extra;
            let (count_narrow, count_wide) = (g.column_count(narrow), g.column_count(wide));
            prop_assert!(
                count_wide >= count_narrow,
                "count dropped from {} to {} when width grew from {} to {}",
                count_narrow, count_wide, narrow, wide
            );
        }
    }

    // ── 5. a negative gap behaves exactly like a zero gap ──
    proptest! {
        #[test]
        fn negative_gap_is_equivalent_to_zero_gap(
            width in arb_width(), min in arb_min_width(), neg_gap in -500.0f32..0.0_f32, max in arb_max_columns(),
        ) {
            let with_negative = adaptive(min, neg_gap, max);
            let with_zero = adaptive(min, 0.0, max);
            prop_assert_eq!(
                with_negative.column_count(width), with_zero.column_count(width),
                "negative gap {} was not clamped the same as zero", neg_gap
            );
            prop_assert!(
                (with_negative.column_width(width) - with_zero.column_width(width)).abs() < 0.01,
                "negative gap {} produced a different column width than zero", neg_gap
            );
        }
    }

    // ── 6. used width never exceeds the available width, clamped or not ──
    proptest! {
        #[test]
        fn used_width_never_exceeds_available_width(
            width in arb_width(), min in arb_min_width(), gap in arb_gap(),
            max_w in arb_max_column_width(), max_columns in arb_max_columns(),
        ) {
            // When `max_w` is `None` this is an algebraic identity: the
            // "stretched" formula in `column_width` is derived exactly from
            // `used_width`'s own equation, so `used_width` reconstructs to
            // `available_width` regardless of which column count was
            // chosen. The interesting case is `max_w: Some(_)` clamping
            // columns narrower than their even share, which is where a
            // genuine `used_width < available_width` gap can open up — both
            // are exercised here since `arb_max_column_width` mixes `None`
            // with concrete clamps.
            let g = ColumnGeometry::from_policy(WidthPolicy::Adaptive { min, max: max_w }, gap, EdgeInsets::ZERO)
                .with_max_columns(max_columns);
            let used = g.used_width(width);
            let available = g.available_width(width);
            prop_assert!(
                used <= available + 0.05,
                "used_width {} exceeds available_width {} at viewport {} (min {} gap {} max_w {:?} max_columns {:?})",
                used, available, width, min, gap, max_w, max_columns
            );
        }
    }

    // ── 7. column_width never exceeds a configured max_column_width ──
    proptest! {
        #[test]
        fn column_width_never_exceeds_the_configured_max_when_set(
            width in arb_width(), min in arb_min_width(), gap in arb_gap(), max_w in 1.0f32..600.0_f32,
        ) {
            let g = ColumnGeometry::from_policy(
                WidthPolicy::Adaptive { min, max: Some(max_w) }, gap, EdgeInsets::ZERO,
            );
            let w = g.column_width(width);
            prop_assert!(
                w <= max_w + 0.01,
                "column_width {} exceeds configured max_column_width {} (viewport {} min {} gap {})",
                w, max_w, width, min, gap
            );
        }
    }
}
