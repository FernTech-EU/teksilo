// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Property tests for the text-run emitter,
//! `teksilo_core::accessibility::text_runs::push_text_runs`
//! (crates/teksilo-core/src/accessibility/text_runs.rs).
//!
//! ## Why this exists
//!
//! Every contract this module upholds is a contract of somebody else's
//! code: `accesskit::Node` panics when a run's `character_lengths` do not
//! sum to its `value`'s byte length, `accesskit_consumer` reads a
//! position's character index through a `u8` (`text.rs:396`, `:443`) so a
//! 256-character run aliases its own end onto index 0, and
//! `Range::bounding_boxes()` throws away every box it has collected the
//! moment one run is missing any of `bounds` / `text_direction` /
//! `character_positions` / `character_widths`. None of those failures is
//! visible from a rendered frame, and only the last one degrades
//! gracefully — the first two crash a screen reader mid-review. The
//! emitter reaches those states through a product of independent
//! choices (line count, wrap width, hard break vs soft wrap, measured vs
//! ellipsized, multi-byte characters, the 255-character chunk boundary),
//! which is exactly the shape example-based tests under-cover.
//!
//! This is an integration test on purpose: the invariants below must hold
//! through the crate's public door, the way a widget author reaches the
//! emitter, not through internals a refactor could quietly re-shape.
//!
//! ## Case counts
//!
//! Every block runs proptest's default 256 cases; none is expensive
//! enough to justify an override, and none is cheap enough to be worth
//! 1024. A deeper one-off run:
//!
//! ```text
//! PROPTEST_CASES=4096 cargo test -p teksilo-core --test text_run_invariants
//! ```
//!
//! Build and run separately, with a memory cap, per
//! `docs/property-testing.md`:
//!
//! ```text
//! cargo test -p teksilo-core --test text_run_invariants --no-run
//! bash -c 'ulimit -v 4000000; cargo test -p teksilo-core --test text_run_invariants'
//! ```

use std::collections::{HashMap, HashSet};

use accesskit::{Node, NodeId, Role};
use proptest::prelude::*;
use teksilo_canvas::{MockTextBackend, Point, TextBackend};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accessibility::text_runs::{MAX_RUN_CHARS, TextRunSource, push_text_runs};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::TextStyle;

/// [`MockTextBackend`]'s advance for every character that has one. Wrap
/// widths below are stated in characters and converted through this, so a
/// generated width means the same thing to the mock and to the test.
const CHAR_WIDTH: f32 = 8.0;

/// The widget the runs hang off. Any id will do — it only seeds the
/// synthetic node ids — but it must be the *same* id at
/// `for_widget` and at `build`, since the second is what resolves the
/// children's window-space bounds.
fn owner() -> WidgetId {
    WidgetId::default()
}

// ── Generators ──────────────────────────────────────────────────────────
//
// Cost: the worst case a generator here can build is `arb_long_text` —
// one `arb_short_text` unit of at most 24 tokens × 2 characters repeated
// until it passes 300 characters, so at most ~350 characters, laid out at
// a wrap width of one character: ~350 single-character lines, each
// emitting one run. That is a few hundred `accesskit::Node`s per case,
// microseconds of work and a few tens of kilobytes. The 255-character cap
// is reached by repetition rather than by generating hundreds of random
// characters, so crossing it costs nothing in generation or in shrinking.

/// One token of generated text.
///
/// Multi-byte characters are in the alphabet because `character_lengths`
/// is measured in bytes while every index around it is measured in
/// characters, and an all-ASCII generator makes the two indistinguishable.
/// `\r\n` is there because AccessKit counts it as one character of length
/// two — the one place a character is not a code point. A lone `\r` is
/// there because it is *not* a break, and the apostrophe and hyphen
/// because UAX #29 keeps them inside a word rather than between two.
fn arb_token() -> impl Strategy<Value = &'static str> {
    prop_oneof![
        6 => Just("a"),
        4 => Just("dit"),
        3 => Just(" "),
        1 => Just("   "),
        2 => Just("é"),
        2 => Just("漢"),
        1 => Just("'"),
        1 => Just("-"),
        1 => Just(","),
        3 => Just("\n"),
        2 => Just("\r\n"),
        1 => Just("\r"),
    ]
}

/// A few dozen characters, freely including leading and trailing spaces
/// and empty strings.
fn arb_short_text() -> impl Strategy<Value = String> {
    prop::collection::vec(arb_token(), 0..24).prop_map(|tokens| tokens.concat())
}

/// A text long enough to cross [`MAX_RUN_CHARS`], built by repeating a
/// short one rather than by generating hundreds of characters: the cap is
/// a property of the length, not of what the characters are, and a
/// repeated unit shrinks to something a human can read.
fn arb_long_text() -> impl Strategy<Value = String> {
    arb_short_text().prop_map(|seed| {
        let unit = if seed.is_empty() {
            "ab é"
        } else {
            seed.as_str()
        };
        let mut out = String::new();
        while out.chars().count() <= MAX_RUN_CHARS + 45 {
            out.push_str(unit);
        }
        out
    })
}

fn arb_source_text() -> impl Strategy<Value = String> {
    prop_oneof![
        8 => arb_short_text(),
        1 => arb_long_text(),
    ]
}

/// How the text reaches the emitter.
#[derive(Debug, Clone, Copy)]
enum Layout {
    /// Painted without a measuring backend: one line, no segments, every
    /// run degenerate. Not an error path — it is what a label drawn
    /// before its first measure reports.
    Flat,
    /// One line, optionally narrower than the text. A width forces the
    /// mock to measure only the drawn prefix, which is how an ellipsized
    /// tail reaches the emitter as text that exists but was never
    /// measured.
    SingleLine(Option<usize>),
    /// Wrapped at the given number of characters, so hard breaks and soft
    /// wraps interleave.
    Paragraph(usize),
}

fn arb_layout() -> impl Strategy<Value = Layout> {
    prop_oneof![
        1 => Just(Layout::Flat),
        2 => Just(Layout::SingleLine(None)),
        2 => (1usize..=12).prop_map(|c| Layout::SingleLine(Some(c))),
        3 => (1usize..=12).prop_map(Layout::Paragraph),
    ]
}

/// What one `push_text_runs` call left behind, as the tree walker sees it.
struct Emitted {
    /// The emitter's own concatenation, which the owning node's `value`
    /// is required to match.
    value: String,
    /// The synthetic `Role::TextRun` children, in emission order.
    runs: Vec<(NodeId, Node)>,
}

/// Lay `text` out and emit its runs through the public door.
fn emit(text: &str, layout: Layout, id_seed: u64) -> Emitted {
    let mut backend = MockTextBackend::new();
    let style = TextStyle::default();
    let measured = match layout {
        Layout::Flat => None,
        Layout::SingleLine(max) => Some(backend.layout_single_line(
            text,
            &style,
            max.map(|chars| chars as f32 * CHAR_WIDTH),
        )),
        Layout::Paragraph(chars) => {
            Some(backend.layout_paragraph(text, &style, chars as f32 * CHAR_WIDTH, None))
        }
    };

    let mut builder = AccessNodeBuilder::for_widget(owner());
    builder.set_role(Role::Label);
    let source = match measured.as_ref().and_then(|l| l.geometry.as_deref()) {
        Some(geometry) => {
            TextRunSource::from_geometry(text, geometry, Point::new(0.0, 0.0), id_seed)
        }
        None => TextRunSource::flat(text, id_seed),
    };
    let value = push_text_runs(&mut builder, None, &source).value;
    let (_, _, runs, _) = builder.build(owner());
    Emitted { value, runs }
}

/// Every run's value, concatenated in emission order — what a consumer
/// reconstructs as the node's document text.
fn concatenated(runs: &[(NodeId, Node)]) -> String {
    runs.iter()
        .map(|(_, node)| node.value().unwrap_or_default())
        .collect()
}

// ── 1. The sum of a run's character lengths is its value's byte length ──
//
// `accesskit::Node`'s own contract, and the one whose violation the
// consumer turns into a panic while slicing (`traverse_text`) rather than
// into a wrong announcement.
proptest! {
    #[test]
    fn sum_of_character_lengths_equals_value_len(
        text in arb_source_text(),
        layout in arb_layout(),
    ) {
        let emitted = emit(&text, layout, 0);
        for (id, node) in &emitted.runs {
            let value = node.value().unwrap_or_default();
            let sum: usize = node.character_lengths().iter().map(|n| *n as usize).sum();
            prop_assert_eq!(
                sum,
                value.len(),
                "run {:?} of {:?} at {:?}: character lengths {:?} sum to {} but the value {:?} \
                 is {} bytes",
                id, text, layout, node.character_lengths(), sum, value, value.len(),
            );
        }
    }
}

// ── 2. Every run carries all four geometry properties, sized to its
//      characters ──
//
// Named for the two that can be short rather than absent, but asserted
// over all four: `Range::bounding_boxes()` discards everything it has
// collected on the first run missing ANY of bounds, direction, positions
// or widths, so one geometry-less run in the middle of a label empties
// the boxes of every range that touches it and a magnifier stops tracking
// the whole label rather than part of it. A positions array *shorter*
// than `character_lengths` is worse still: the consumer indexes it
// directly (`bounding_boxes`, `character_index_at_point`).
proptest! {
    #[test]
    fn positions_and_widths_are_always_present_and_as_long_as_lengths(
        text in arb_source_text(),
        layout in arb_layout(),
    ) {
        let emitted = emit(&text, layout, 0);
        for (id, node) in &emitted.runs {
            let expected = node.character_lengths().len();
            let positions = node.character_positions();
            let widths = node.character_widths();
            prop_assert!(
                positions.is_some_and(|p| p.len() == expected),
                "run {:?} of {:?} at {:?}: {} characters but character_positions {:?}",
                id, text, layout, expected, positions,
            );
            prop_assert!(
                widths.is_some_and(|w| w.len() == expected),
                "run {:?} of {:?} at {:?}: {} characters but character_widths {:?}",
                id, text, layout, expected, widths,
            );
            prop_assert!(
                node.bounds().is_some(),
                "run {:?} of {:?} at {:?} has no bounds",
                id, text, layout,
            );
            prop_assert!(
                node.text_direction().is_some(),
                "run {:?} of {:?} at {:?} has no text direction",
                id, text, layout,
            );
        }
    }
}

// ── 3. Word starts are sorted, strictly increasing, and inside the run ──
//
// `accesskit_consumer::Position::forward_to_word_start` binary-searches
// `word_starts` for `character_index as u8` (`text.rs:396`) and indexes
// straight into the run at whatever it finds, so an unsorted list makes
// word navigation skip, a duplicate makes it stall, and an entry at or
// past the run's character count indexes out of the run.
proptest! {
    #[test]
    fn word_starts_are_sorted_strictly_increasing_and_below_255(
        text in arb_source_text(),
        layout in arb_layout(),
    ) {
        let emitted = emit(&text, layout, 0);
        for (id, node) in &emitted.runs {
            let starts = node.word_starts();
            let characters = node.character_lengths().len();
            for pair in starts.windows(2) {
                prop_assert!(
                    pair[0] < pair[1],
                    "run {:?} of {:?} at {:?}: word starts {:?} are not strictly increasing",
                    id, text, layout, starts,
                );
            }
            for &start in starts {
                prop_assert!(
                    (start as usize) < characters,
                    "run {:?} of {:?} at {:?}: word start {} is not inside a run of {} \
                     characters (word starts {:?})",
                    id, text, layout, start, characters, starts,
                );
                prop_assert!(
                    (start as usize) < MAX_RUN_CHARS,
                    "run {:?} of {:?} at {:?}: word start {} is at or past the 255-character \
                     index the consumer can express",
                    id, text, layout, start,
                );
            }
        }
    }
}

// ── 4. No run exceeds the 255-character cap ──
//
// The consumer stores a position's character index in a `u8`, so a run of
// 256 characters aliases its end onto index 0 and word navigation walks
// backwards through text it has already read.
proptest! {
    #[test]
    fn no_run_exceeds_255_characters(
        text in arb_source_text(),
        layout in arb_layout(),
    ) {
        // 255 is spelled out rather than taken from `MAX_RUN_CHARS`: it is
        // AccessKit's limit, not a Teksilo tuning knob — `word_starts` is a
        // `Vec<u8>` and the consumer probes `character_index as u8`
        // (`accesskit_consumer-0.39.0/src/text.rs:396`, `:443`), so a run of
        // 256 aliases its end onto index 0. Asserting against the constant
        // would make this test follow a mistaken change to it.
        const ACCESSKIT_CAP: usize = 255;
        prop_assert_eq!(
            MAX_RUN_CHARS, ACCESSKIT_CAP,
            "the emitter's cap must stay at AccessKit's own limit"
        );
        let emitted = emit(&text, layout, 0);
        for (id, node) in &emitted.runs {
            prop_assert!(
                node.character_lengths().len() <= ACCESSKIT_CAP,
                "run {:?} of a {}-character text at {:?} carries {} characters, past the \
                 {}-character cap",
                id, text.chars().count(), layout, node.character_lengths().len(), ACCESSKIT_CAP,
            );
        }
    }
}

// ── 5. The same-line chain is symmetric and never dangles ──
//
// `InnerPosition::line_start` / `line_end` follow the chain with an
// `unwrap()` on the looked-up node, so a link to a run that never reached
// the tree panics the consumer outright; an asymmetric link makes a line
// walk that arrives from one side disagree with one that arrives from the
// other.
proptest! {
    #[test]
    fn next_and_previous_on_line_are_symmetric_and_never_dangle(
        text in arb_source_text(),
        layout in arb_layout(),
    ) {
        let emitted = emit(&text, layout, 0);
        let by_id: HashMap<NodeId, &Node> =
            emitted.runs.iter().map(|(id, node)| (*id, node)).collect();
        for (id, node) in &emitted.runs {
            if let Some(next) = node.next_on_line() {
                let target = by_id.get(&next);
                prop_assert!(
                    target.is_some(),
                    "run {:?} of {:?} at {:?} points next_on_line at {:?}, which was never \
                     emitted",
                    id, text, layout, next,
                );
                prop_assert_eq!(
                    target.unwrap().previous_on_line(),
                    Some(*id),
                    "run {:?} of {:?} at {:?} points next_on_line at {:?}, which does not point \
                     back",
                    id, text, layout, next,
                );
            }
            if let Some(previous) = node.previous_on_line() {
                let target = by_id.get(&previous);
                prop_assert!(
                    target.is_some(),
                    "run {:?} of {:?} at {:?} points previous_on_line at {:?}, which was never \
                     emitted",
                    id, text, layout, previous,
                );
                prop_assert_eq!(
                    target.unwrap().next_on_line(),
                    Some(*id),
                    "run {:?} of {:?} at {:?} points previous_on_line at {:?}, which does not \
                     point back",
                    id, text, layout, previous,
                );
            }
        }
    }
}

// ── 6. The runs reconstruct the source text exactly ──
//
// A consumer derives the node's document text by concatenating its runs,
// while every platform announces the node's `value`. Any divergence is a
// place where what a reader reviews character by character and what it
// hears are different strings — and a soft wrap, which adds nothing to
// the text, is the easiest way to introduce one.
proptest! {
    #[test]
    fn concat_of_run_values_equals_the_source_text(
        text in arb_source_text(),
        layout in arb_layout(),
    ) {
        let emitted = emit(&text, layout, 0);
        prop_assert_eq!(
            &emitted.value,
            &text,
            "at {:?}: the emitter's own value diverges from the source",
            layout,
        );
        prop_assert_eq!(
            concatenated(&emitted.runs),
            text.clone(),
            "at {:?}: the {} emitted runs do not reconstruct the source",
            layout,
            emitted.runs.len(),
        );
    }
}

// ── 7. Character positions never move backwards inside a run ──
//
// Positions are measured from the run's leading edge — its left edge in
// LTR, its right edge in RTL — so they are non-decreasing in both
// directions. `bounding_boxes` subtracts a start position from an end
// position to size the box; a position that moves backwards produces a
// negative-width rectangle, which reads as an empty selection to the
// magnifier that asked.
proptest! {
    #[test]
    fn character_positions_are_non_decreasing_within_a_run(
        text in arb_source_text(),
        layout in arb_layout(),
    ) {
        let emitted = emit(&text, layout, 0);
        for (id, node) in &emitted.runs {
            let positions = node.character_positions().unwrap_or_default();
            for pair in positions.windows(2) {
                prop_assert!(
                    pair[0] <= pair[1],
                    "run {:?} of {:?} at {:?}: positions {:?} move backwards",
                    id, text, layout, positions,
                );
            }
        }
    }
}

// ── 8. Ids stay distinct across every source in one builder ──
//
// Two children sharing a `NodeId` panic `accesskit_consumer`'s tree
// builder. A run's id is derived from (byte offset, byte length,
// `id_seed`), so two sources with identical text — an editor's two
// identical blocks, a terminal's two identical rows — collide on
// everything but the seed, and the seed is the whole defence. The
// concatenation is asserted alongside because a collision is *resolved*
// by dropping the second run, which would otherwise leave the ids
// trivially distinct and the text silently short.
proptest! {
    #[test]
    fn run_ids_are_unique_across_sources_in_one_builder(
        texts in prop::collection::vec(arb_short_text(), 1..4),
    ) {
        // The first text again under its own seed: same offsets, same
        // lengths, different seed.
        let mut sources = texts.clone();
        sources.push(texts[0].clone());

        let mut builder = AccessNodeBuilder::for_widget(owner());
        builder.set_role(Role::Label);
        let mut expected = String::new();
        for (id_seed, text) in sources.iter().enumerate() {
            let source = TextRunSource::flat(text, id_seed as u64);
            expected.push_str(&push_text_runs(&mut builder, None, &source).value);
        }
        let (_, _, runs, _) = builder.build(owner());

        let ids: HashSet<NodeId> = runs.iter().map(|(id, _)| *id).collect();
        prop_assert_eq!(
            ids.len(),
            runs.len(),
            "{:?} emitted {} runs but only {} distinct ids",
            sources,
            runs.len(),
            ids.len(),
        );
        prop_assert_eq!(
            concatenated(&runs),
            expected,
            "{:?}: the emitted runs do not reconstruct the concatenated sources",
            sources,
        );
    }
}
