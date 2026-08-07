// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Minimal headless reproduction of the spell-checked editor freeze.
//!
//! Skribisto reports that opening a ~13k-word Lorem-Ipsum scene with spell-check
//! on (one flagged range per word, ~10k+ `RangeHighlight`s) pins a Ryzen 9 and
//! freezes the app — in a *release* build, while merely focused (caret blinking),
//! with no editing. Two independent code maps produced a testable disagreement:
//!
//!   * The render map claimed every blink-driven redraw re-pays O(whole document)
//!     even though no dirty flag misfires: `sync_accessibility`'s cache-*hit* still
//!     `.clone()`s the entire `TreeUpdate`, and `render_cursor_only` touches ALL
//!     glyphs + retains ALL ~10k decorations — amplified by "dubious mode"
//!     (`ScrollPolicy::AlwaysOff`, full-height editor, no viewport culling).
//!   * The a11y map claimed the cache-hit clone is *cheap*; the expensive part —
//!     `extract_paint_spans`, suspected O(m²) per block — only recurs on an edit
//!     or a fresh spell re-scan, NOT on a bare caret blink, and scales with how
//!     concentrated the ranges are within a single block.
//!
//! This profile settles it by measuring, for a clean vs a 10k-range document in
//! both a *spread* (many small paragraphs) and a *concentrated* (one giant
//! paragraph) shape, four costs on ONE focused `RichTextEditor`:
//!
//!   cold    — first real paint + first a11y build (cache miss)
//!   warm    — an immediate no-op paint + a11y sync (must both be near-zero:
//!             nothing changed, so both hit their caches)
//!   blink   — a caret-blink-driven repaint + the following a11y sync (steady state)
//!   rescan  — a repaint + a11y sync right after re-pushing the ranges (rebuild)
//!
//! The ratios loaded÷clean, and spread vs concentrated, name the culprit exactly.
//! Run it with output shown:
//!
//!   cargo test -p teksilo-widgets --release --test editor_freeze_repro -- --nocapture
//!
//! Frames are dropped the instant they are timed, so a cache-hit render stays a
//! cache hit (holding a second `Rc` would force `render()`'s `Rc::make_mut` into
//! a deep clone and mismeasure it). `advance_to_blink_repaint` guarantees a real
//! repaint is pending before the BLINK sample, so a blink that reads ~0 genuinely
//! means the blink path is cheap — not that the sample missed the toggle. Times
//! are printed, never asserted, so this never flakes on a shared runner.

use std::time::{Duration, Instant};

use teksilo_canvas::SizeProposal;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::{
    Color, HighlightFormat, HighlightMask, RangeHighlight, TextDocument, UnderlineStyle,
};
use teksilo_widgets::rich_text::{RichTextEditor, ScrollPolicy};

/// The outer viewport width.
const WIDTH: f32 = 900.0;

/// The viewport HEIGHT handed to the editor. Deliberately taller than any
/// 13k-word document, so the editor's viewport ⊇ its content and text-typeset's
/// culling (`view_bottom = scroll + viewport_height`) never trims anything — the
/// exact "dubious mode" Skribisto creates by giving an `AlwaysOff` editor a
/// full-height slot inside an outer `ScrollArea`. A width-only proposal instead
/// lets the editor settle at a small intrinsic height, which culls the render and
/// hides the very cost under test.
const HEIGHT: f32 = 200_000.0;

const WORD: &str = "lorem";

/// The wavy spell-check underline, verbatim from `spellcheck.rs::spell_format`:
/// a colour/underline-only format, so `compute_range_kind` classifies it
/// `PaintOnly` — it recolours without reshaping and stays out of the AT tree's
/// fragments, exactly like the production squiggle.
fn spell_format() -> HighlightFormat {
    HighlightFormat {
        underline_style: Some(UnderlineStyle::SpellCheckUnderline),
        underline_color: Some(Color {
            red: 220,
            green: 50,
            blue: 50,
            alpha: 255,
        }),
        ..Default::default()
    }
}

/// One flagged range per whitespace-delimited word. The fixtures are pure ASCII,
/// so a byte offset equals a char offset and `char_indices` yields the exact
/// block-absolute `start`/`length` the range API wants — the same one-per-word
/// shape `SpellSession::rebuild_all_ranges` produces on a mis-dictionaried doc.
fn word_ranges(text: &str) -> Vec<RangeHighlight> {
    let fmt = spell_format();
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, ch) in text.char_indices() {
        if ch == ' ' || ch == '\n' {
            if let Some(s) = start.take() {
                out.push(RangeHighlight {
                    start: s,
                    length: i - s,
                    format: fmt.clone(),
                });
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push(RangeHighlight {
            start: s,
            length: text.len() - s,
            format: fmt.clone(),
        });
    }
    out
}

/// `paras` paragraphs of `words_per_para` words each, blank-line separated — the
/// realistic multi-scene shape where each range buckets into a small block.
fn spread_doc(paras: usize, words_per_para: usize) -> String {
    let para = vec![WORD; words_per_para].join(" ");
    vec![para; paras].join("\n\n")
}

/// One paragraph of `words` words — the "single giant Lorem scene" shape, where
/// every range lands in ONE block. This is the case that turns `extract_paint_spans`
/// quadratic if it is O(spans-in-block²).
fn concentrated_doc(words: usize) -> String {
    vec![WORD; words].join(" ")
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Time one full-frame paint, dropping the frame immediately so the tree stays
/// its sole owner (a held `Rc` forces `render()`'s `Rc::make_mut` to deep-clone).
fn render_ms(tree: &mut WidgetTree) -> f64 {
    let t = Instant::now();
    let _ = tree.render();
    ms(t.elapsed())
}

/// Time one accessibility sync, dropping the `TreeUpdate` immediately.
fn a11y_ms(tree: &mut WidgetTree) -> f64 {
    let t = Instant::now();
    let _ = tree.sync_accessibility();
    ms(t.elapsed())
}

/// One simulated frame: request a tick, advance the sim clock (which fires the
/// frame-tick effect → `frame_loop::tick` → `CaretBlink::tick`), and re-lay-out
/// at the full-height (dubious-mode) viewport. Mirrors `rich_text/tests.rs::tick_once`.
fn tick(tree: &mut WidgetTree) {
    tree.request_frame();
    tree.tick_animations(Duration::from_millis(16));
    tree.layout(SizeProposal::exact(WIDTH, HEIGHT));
}

/// `CaretBlink` reads real wall-clock time (0.5 s interval), so drive real frames
/// until a blink toggle marks the editor for an actual repaint. `needs_render()`
/// is precisely `any_needs_layout || any_needs_paint` (unlike `needs_redraw`,
/// which also fires on a bare frame-tick request), so this returns only once a
/// genuine paint is pending. Panics if it never blinks, so the harness can never
/// silently time a cache hit and under-report the blink cost.
fn advance_to_blink_repaint(tree: &mut WidgetTree) {
    for _ in 0..12 {
        std::thread::sleep(Duration::from_millis(550));
        tick(tree);
        if tree.needs_render() {
            return;
        }
    }
    panic!("the caret blink never marked the editor for repaint in 12 ticks");
}

struct Row {
    label: &'static str,
    n_ranges: usize,
    content_h: f32,
    render_cold: f64,
    a11y_snapshot: f64,
    a11y_cold: f64,
    render_warm: f64,
    a11y_warm: f64,
    render_blink: f64,
    a11y_blink: f64,
    render_rescan: Option<f64>,
    a11y_rescan: Option<f64>,
}

fn run_config(label: &'static str, text: &str, load: bool) -> Row {
    let doc = TextDocument::new();
    doc.set_plain_text(text).unwrap();

    // Install the ranges up front — the flow snapshot reads live session state,
    // so they are reflected in layout / paint / a11y from the first frame,
    // exactly like a doc that opened already spell-checked.
    let session = load.then(|| doc.add_range_session());
    let n_ranges = if let Some(s) = session {
        let ranges = word_ranges(text);
        let n = ranges.len();
        doc.set_session_ranges(s, ranges);
        n
    } else {
        0
    };

    let mut tree = WidgetTree::new();
    let editor = RichTextEditor::editor(doc.clone())
        .v_scroll_policy(ScrollPolicy::AlwaysOff)
        .h_scroll_policy(ScrollPolicy::AlwaysOff);
    let id = tree.add(editor);
    tree.focus(id);

    // Settle: build, drain the initial set_plain_text burst, run the full layout.
    for _ in 0..4 {
        tick(&mut tree);
    }
    let content_h = tree.bounds(id).height;

    // COLD — first genuine paint (Full) and first a11y build (cache miss).
    let render_cold = render_ms(&mut tree);
    // The a11y SNAPSHOT alone (no-paint), to split the a11y build into
    // "snapshot" vs "walk (character_geometry etc.)". Measured against the same
    // engine layout the real a11y build sees.
    let a11y_snapshot = {
        let t = Instant::now();
        let _ = doc.snapshot_flow_masked_no_paint(&HighlightMask::all());
        ms(t.elapsed())
    };
    let a11y_cold = a11y_ms(&mut tree);

    // WARM — immediate repeat with nothing changed. `needs_render()` should be
    // false now, so both calls hit their caches; if either warm number is large
    // it means something re-dirties every frame all on its own (the bug, made
    // visible without any blink).
    let render_warm = render_ms(&mut tree);
    let a11y_warm = a11y_ms(&mut tree);

    // BLINK — a caret toggle (RepaintOnly) marks needs_paint but leaves a11y
    // clean. advance_to_blink_repaint guarantees a real repaint is pending.
    advance_to_blink_repaint(&mut tree);
    let render_blink = render_ms(&mut tree);
    let a11y_blink = a11y_ms(&mut tree);

    // RESCAN — re-push the ranges (as a live spellchecker re-scan would). Fires
    // HighlightPaintChanged → invalidates the a11y snapshot + requests a
    // recolour, so the following sync_accessibility is a genuine rebuild.
    let (render_rescan, a11y_rescan) = if let Some(s) = session {
        doc.set_session_ranges(s, word_ranges(text));
        tick(&mut tree);
        let tr = render_ms(&mut tree);
        let ta = a11y_ms(&mut tree);
        (Some(tr), Some(ta))
    } else {
        (None, None)
    };

    Row {
        label,
        n_ranges,
        content_h,
        render_cold,
        a11y_snapshot,
        a11y_cold,
        render_warm,
        a11y_warm,
        render_blink,
        a11y_blink,
        render_rescan,
        a11y_rescan,
    }
}

fn opt(v: Option<f64>) -> String {
    v.map(|x| format!("{x:9.2}"))
        .unwrap_or_else(|| "        —".to_string())
}

// A manual perf harness, not a CI guard: it prints per-frame timings and drives
// real-time caret-blink waits (~5s of `thread::sleep`), so it is #[ignore]d and
// run on demand with `cargo test ... -- --ignored --nocapture`. The correctness
// of the fixes it measures is guarded by the differential / no-paint / monotonic
// unit tests in text-document and text-typeset.
#[test]
#[ignore = "manual perf harness (sleeps ~5s, prints timings) — run with --ignored --nocapture"]
fn editor_freeze_profile() {
    // ~13k words each — the reported scene size.
    let configs: [(&'static str, String, bool); 4] = [
        ("spread/clean       ", spread_doc(277, 47), false),
        ("spread/10k-ranges  ", spread_doc(277, 47), true),
        ("concentrat./clean  ", concentrated_doc(13_000), false),
        ("concentrat./10k    ", concentrated_doc(13_000), true),
    ];

    let rows: Vec<Row> = configs
        .iter()
        .map(|(label, text, load)| run_config(label, text, *load))
        .collect();

    eprintln!();
    eprintln!("editor freeze profile — all times in ms, one focused RichTextEditor, dubious mode");
    eprintln!(
        "{:<20} {:>7} {:>8} | {:>9} {:>9} | {:>9} {:>9} | {:>9} {:>9} | {:>9} {:>9}",
        "config",
        "ranges",
        "content",
        "render",
        "a11y",
        "render",
        "a11y",
        "render",
        "a11y",
        "render",
        "a11y",
    );
    eprintln!(
        "{:<20} {:>7} {:>8} | {:>9} {:>9} | {:>9} {:>9} | {:>9} {:>9} | {:>9} {:>9}",
        "", "", "px", "COLD", "COLD", "warm", "warm", "BLINK", "BLINK", "RESCAN", "RESCAN",
    );
    for r in &rows {
        eprintln!(
            "{:<20} {:>7} {:>8.0} | {:>9.2} {:>9.2} | {:>9.2} {:>9.2} | {:>9.2} {:>9.2} | {} {}",
            r.label,
            r.n_ranges,
            r.content_h,
            r.render_cold,
            r.a11y_cold,
            r.render_warm,
            r.a11y_warm,
            r.render_blink,
            r.a11y_blink,
            opt(r.render_rescan),
            opt(r.a11y_rescan),
        );
    }
    eprintln!();
    eprintln!(
        "a11y COLD split — snapshot (no-paint) vs the rest of the build (walk/geometry/diff):"
    );
    for r in &rows {
        eprintln!(
            "  {:<20} a11y_snapshot={:>8.2}   a11y_build_total={:>8.2}   walk≈{:>8.2}",
            r.label,
            r.a11y_snapshot,
            r.a11y_cold,
            (r.a11y_cold - r.a11y_snapshot).max(0.0),
        );
    }
    eprintln!();

    // Print the decisive ratios (loaded ÷ clean) for the same doc shape.
    let ratio = |num: f64, den: f64| if den > 0.0 { num / den } else { f64::NAN };
    let (spread_clean, spread_10k) = (&rows[0], &rows[1]);
    let (conc_clean, conc_10k) = (&rows[2], &rows[3]);
    eprintln!("loaded ÷ clean ratios (>1 ⇒ that cost scales with the 10k spell ranges):");
    eprintln!(
        "  spread:       blink render {:>7.1}×   blink a11y {:>7.1}×   rescan a11y {:>7.1}×",
        ratio(spread_10k.render_blink, spread_clean.render_blink),
        ratio(spread_10k.a11y_blink, spread_clean.a11y_blink),
        ratio(
            spread_10k.a11y_rescan.unwrap_or(f64::NAN),
            spread_clean.a11y_cold
        ),
    );
    eprintln!(
        "  concentrated: blink render {:>7.1}×   blink a11y {:>7.1}×   rescan a11y {:>7.1}×",
        ratio(conc_10k.render_blink, conc_clean.render_blink),
        ratio(conc_10k.a11y_blink, conc_clean.a11y_blink),
        ratio(
            conc_10k.a11y_rescan.unwrap_or(f64::NAN),
            conc_clean.a11y_cold
        ),
    );
    eprintln!(
        "concentrated ÷ spread rescan a11y (>1 ⇒ extract_paint_spans is superlinear per block): {:>7.1}×",
        ratio(
            conc_10k.a11y_rescan.unwrap_or(f64::NAN),
            spread_10k.a11y_rescan.unwrap_or(f64::NAN)
        ),
    );
    eprintln!();
}
