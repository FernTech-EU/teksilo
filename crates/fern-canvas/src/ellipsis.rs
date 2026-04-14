//! Middle and leading ellipsis truncation helpers.
//!
//! Given a text, style, and pixel width budget, return the display string
//! that should be rendered — with the ellipsis character `"\u{2026}"`
//! already inserted at the correct position.
//!
//! Trailing ellipsis does not live here: text-typeset's
//! `layout_single_line` already appends an ellipsis when the shaped
//! advance exceeds the supplied `max_width`. Callers wanting trailing
//! behavior just hand the width budget to `layout_single_line` directly.
//! This module handles only the two modes text-typeset can't do for us.
//!
//! The algorithms are char-iteration based (not full grapheme-cluster
//! segmentation) — adequate for typical UI labels, to be upgraded to
//! `unicode-segmentation` graphemes when emoji / combining mark edge
//! cases become visible.

use fern_tokens::TextStyle;

use crate::text_backend::{EllipsisMode, TextBackend};

pub const ELLIPSIS: char = '\u{2026}';

/// Produce the truncated display string for `mode` so the resulting
/// shaped width is ≤ `max_width`.
///
/// For [`EllipsisMode::Trailing`] this returns `text.to_string()`
/// unchanged — the caller should pass the returned string to
/// `layout_single_line(..., Some(max_width))` so text-typeset's own
/// trailing truncation runs.
///
/// For [`EllipsisMode::Middle`] and [`EllipsisMode::Leading`] this walks
/// the text in char increments, re-measuring via `backend`, and returns
/// the longest substring that fits. If even a bare `"\u{2026}"` exceeds
/// the budget, returns a single ellipsis character.
pub fn ellipsize(
    text: &str,
    style: &TextStyle,
    max_width: f32,
    mode: EllipsisMode,
    backend: &mut dyn TextBackend,
) -> String {
    if text.is_empty() || max_width <= 0.0 {
        return String::new();
    }

    // Fast path: full text already fits.
    let full_width = measure(text, style, backend);
    if full_width <= max_width {
        return text.to_string();
    }

    match mode {
        EllipsisMode::Trailing => text.to_string(),
        EllipsisMode::Middle => middle_ellipsize(text, style, max_width, backend),
        EllipsisMode::Leading => leading_ellipsize(text, style, max_width, backend),
    }
}

fn measure(text: &str, style: &TextStyle, backend: &mut dyn TextBackend) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    backend.layout_single_line(text, style, None).width
}

fn ellipsis_str() -> String {
    let mut s = String::new();
    s.push(ELLIPSIS);
    s
}

/// Middle ellipsis: `"Lorem…sit amet"`.
///
/// Strategy: collect char byte-boundaries, then search the largest
/// `total_kept` such that `head(first head_len chars) + "…" + tail(last
/// tail_len chars)` fits under `max_width`, with `head_len` and
/// `tail_len` split as evenly as possible (head gets the extra char for
/// odd totals). Linear scan from `n` downward — O(n) iterations, each
/// doing one measurement.
fn middle_ellipsize(
    text: &str,
    style: &TextStyle,
    max_width: f32,
    backend: &mut dyn TextBackend,
) -> String {
    let boundaries: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect();
    let n = boundaries.len().saturating_sub(1); // number of chars

    if n == 0 {
        return String::new();
    }

    // Walk down from keeping n-1 chars to keeping 0. Skip `n` because we
    // already know the full text doesn't fit.
    for total_kept in (0..n).rev() {
        let head_len = total_kept.div_ceil(2);
        let tail_len = total_kept - head_len;
        let head_end_byte = boundaries[head_len];
        let tail_start_byte = boundaries[n - tail_len];

        let mut candidate = String::with_capacity(head_end_byte + 4 + (text.len() - tail_start_byte));
        candidate.push_str(&text[..head_end_byte]);
        candidate.push(ELLIPSIS);
        candidate.push_str(&text[tail_start_byte..]);

        if measure(&candidate, style, backend) <= max_width {
            return candidate;
        }
    }

    // Nothing fit — the budget is smaller than a bare ellipsis.
    ellipsis_str()
}

/// Leading ellipsis: `"…dolor sit amet"`.
///
/// Strategy: try increasingly deeper cut points from the start — for
/// each char boundary `start`, the candidate is `"…" + text[start..]`.
/// Return the first candidate that fits, which is also the longest
/// (preserves as much trailing content as possible).
fn leading_ellipsize(
    text: &str,
    style: &TextStyle,
    max_width: f32,
    backend: &mut dyn TextBackend,
) -> String {
    let boundaries: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect();

    if boundaries.len() <= 1 {
        return String::new();
    }

    // Try cutting 1 char, then 2, etc. from the start. `start = 0`
    // means keeping all text (we already know that doesn't fit).
    for &start_byte in boundaries.iter().skip(1) {
        let mut candidate = String::with_capacity(4 + (text.len() - start_byte));
        candidate.push(ELLIPSIS);
        candidate.push_str(&text[start_byte..]);

        if measure(&candidate, style, backend) <= max_width {
            return candidate;
        }
    }

    ellipsis_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text_backend::MockTextBackend;

    fn style() -> TextStyle {
        TextStyle::default()
    }

    // MockTextBackend uses 8px per char — predictable for assertions.

    #[test]
    fn short_text_fits_unchanged() {
        let mut backend = MockTextBackend::new();
        let s = ellipsize("Hi", &style(), 100.0, EllipsisMode::Middle, &mut backend);
        assert_eq!(s, "Hi");
    }

    #[test]
    fn trailing_is_shim_passthrough() {
        let mut backend = MockTextBackend::new();
        // Long text, trailing mode — returns original (caller is expected
        // to forward max_width to layout_single_line).
        let s = ellipsize(
            "Hello World",
            &style(),
            20.0,
            EllipsisMode::Trailing,
            &mut backend,
        );
        assert_eq!(s, "Hello World");
    }

    #[test]
    fn middle_inserts_ellipsis_and_preserves_ends() {
        let mut backend = MockTextBackend::new();
        // 21 chars × 8px = 168px. Budget 80px → keeps ~10 chars + "…".
        let s = ellipsize(
            "Hello beautiful world",
            &style(),
            80.0,
            EllipsisMode::Middle,
            &mut backend,
        );
        assert!(s.contains(ELLIPSIS), "expected ellipsis in {s:?}");
        assert!(s.starts_with('H'), "head preserved in {s:?}");
        assert!(s.ends_with('d'), "tail preserved in {s:?}");
        // 8px-per-char width check with 1px slack for the ellipsis
        // measurement (also 8px in the mock).
        let w = measure(&s, &style(), &mut backend);
        assert!(w <= 80.0, "width {w} exceeds budget 80");
    }

    #[test]
    fn leading_starts_with_ellipsis() {
        let mut backend = MockTextBackend::new();
        let s = ellipsize(
            "Hello beautiful world",
            &style(),
            80.0,
            EllipsisMode::Leading,
            &mut backend,
        );
        assert!(s.starts_with(ELLIPSIS), "expected leading ellipsis in {s:?}");
        assert!(s.ends_with('d'), "tail preserved in {s:?}");
        let w = measure(&s, &style(), &mut backend);
        assert!(w <= 80.0, "width {w} exceeds budget 80");
    }

    #[test]
    fn empty_text_returns_empty() {
        let mut backend = MockTextBackend::new();
        let s = ellipsize("", &style(), 100.0, EllipsisMode::Middle, &mut backend);
        assert_eq!(s, "");
    }

    #[test]
    fn tiny_budget_returns_bare_ellipsis() {
        let mut backend = MockTextBackend::new();
        // Budget smaller than a single char — mock's ellipsis char is 8px,
        // budget is 4px, so nothing can fit.
        let s = ellipsize(
            "Hello",
            &style(),
            4.0,
            EllipsisMode::Middle,
            &mut backend,
        );
        // Either empty (budget < ellipsis) or bare "…" — both acceptable.
        assert!(s.is_empty() || s == ellipsis_str());
    }

    #[test]
    fn leading_picks_longest_fitting_suffix() {
        let mut backend = MockTextBackend::new();
        // MockTextBackend measures by UTF-8 byte length (8px per byte).
        // "…" is 3 bytes in UTF-8, so a bare "…" measures 24px. At budget
        // 40 we have 16 bytes of tail headroom, i.e. 2 ASCII chars.
        // Longest fitting suffix of "ABCDEFGHIJ" is "IJ" → "…IJ" = 40px.
        let s = ellipsize(
            "ABCDEFGHIJ",
            &style(),
            40.0,
            EllipsisMode::Leading,
            &mut backend,
        );
        assert_eq!(s, "\u{2026}IJ");
    }
}
