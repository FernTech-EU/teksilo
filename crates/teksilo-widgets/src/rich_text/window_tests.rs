// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `window_to_clip` — the dubious-mode render-window wiring.
//!
//! Phase A made the accessibility rebuild cheap; this is Phase B, cutting the
//! *render*. An editor laid out at full document height inside an outer
//! `ScrollArea` reads the accumulated ancestor clip from `PaintContext` and feeds
//! text-typeset a render window, so a huge document only rasterizes the rows on
//! screen. These tests pin the wiring: the window matches the clip, a plain
//! editor sets none, a scroll moves the window (the reactivity that makes it
//! correct without any ScrollArea→editor plumbing), and nested ScrollAreas still
//! bound it.

use teksilo_canvas::SizeProposal;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::TextDocument;

use super::state::SharedState;
use super::{RichTextEditor, ScrollPolicy};
use crate::ScrollArea;

const W: f32 = 900.0;
const H: f32 = 700.0;

/// One huge paragraph — many wrapped lines, far taller than the viewport.
fn giant_doc() -> TextDocument {
    let text = vec!["lorem"; 6000].join(" ");
    let doc = TextDocument::new();
    doc.set_plain_text(&text).unwrap();
    doc
}

fn make_editor(window: bool) -> RichTextEditor {
    RichTextEditor::editor(giant_doc())
        .v_scroll_policy(ScrollPolicy::AlwaysOff)
        .h_scroll_policy(ScrollPolicy::AlwaysOff)
        // `min_lines` switches to INTRINSIC sizing, so the editor reports its full
        // document height inside the outer ScrollArea (dubious mode) — the whole
        // point. Without it a greedy editor just fills the 700px viewport and there
        // is nothing to window or scroll.
        .min_lines(1)
        .window_to_clip(window)
}

/// Drive a few frames and paint, at the given outer viewport size.
fn settle(tree: &mut WidgetTree, w: f32, h: f32) {
    for _ in 0..4 {
        tree.request_frame();
        tree.tick_animations(std::time::Duration::from_millis(16));
        tree.layout(SizeProposal::exact(w, h));
    }
    tree.render();
}

/// Glyphs in the editor's *current* (culled) frame — a cursor-only render reuses
/// the cached glyphs from the last full render without rebuilding them.
fn glyph_count(state: &SharedState) -> usize {
    state
        .borrow_mut()
        .engine
        .with_render_cursor_only(|f| f.glyphs.len())
}

fn render_window(state: &SharedState) -> Option<(f32, f32)> {
    state.borrow().engine.render_window()
}

#[test]
fn window_to_clip_culls_to_the_visible_band() {
    // Windowed editor inside a 700px-tall ScrollArea.
    let ed = make_editor(true);
    let state = ed.state_handle();
    let mut tree = WidgetTree::new();
    let id = tree.add(ed);
    tree.add(ScrollArea::from_id(id));
    settle(&mut tree, W, H);

    let win = render_window(&state).expect("a windowed editor must set a render window");
    assert!(
        win.1 < H * 3.0,
        "the window height ({}) must be viewport-scale (≈{H}px + margin), not the whole \
         document",
        win.1
    );
    let windowed = glyph_count(&state);
    assert!(windowed > 0, "the visible band must render something");

    // Same document + layout, but NOT windowed → the whole document renders.
    let ed2 = make_editor(false);
    let state2 = ed2.state_handle();
    let mut tree2 = WidgetTree::new();
    let id2 = tree2.add(ed2);
    tree2.add(ScrollArea::from_id(id2));
    settle(&mut tree2, W, H);

    assert_eq!(
        render_window(&state2),
        None,
        "a non-windowed editor must not set a render window"
    );
    let full = glyph_count(&state2);
    assert!(
        windowed * 3 < full,
        "windowing must cull the bulk of the document: windowed={windowed} vs full={full}"
    );
}

#[test]
fn scrolling_the_outer_area_moves_the_render_window() {
    // The reactivity lynchpin of the clip-based design: a scroll re-places and
    // re-paints the editor (ScrollArea binds scroll_y at Relayout), so the window
    // recomputes from the fresh bounds — no ScrollArea→editor wiring involved.
    let ed = make_editor(true);
    let state = ed.state_handle();
    let mut tree = WidgetTree::new();
    let id = tree.add(ed);
    let area = ScrollArea::from_id(id);
    let scroll = area.scroll_y_signal().clone();
    tree.add(area);
    settle(&mut tree, W, H);

    let top_before = render_window(&state).unwrap().0;

    scroll.set(1500.0);
    settle(&mut tree, W, H);
    let top_after = render_window(&state).unwrap().0;

    assert!(
        top_after > top_before + 500.0,
        "scrolling down 1500px must advance the render-window top ({top_before} → {top_after})"
    );
}

#[test]
fn the_render_window_tracks_the_visible_clip_band() {
    // With window_to_clip, the cull band height tracks the on-screen clip
    // (plus margin). Font-size scale grows metrics but does not shrink the
    // windowed height the way page-zoom once did.
    let ed = RichTextEditor::editor(giant_doc())
        .v_scroll_policy(ScrollPolicy::AlwaysOff)
        .h_scroll_policy(ScrollPolicy::AlwaysOff)
        .min_lines(1)
        .font_size_scale(1.5)
        .window_to_clip(true);
    let state = ed.state_handle();
    let mut tree = WidgetTree::new();
    let id = tree.add(ed);
    tree.add(ScrollArea::from_id(id));
    settle(&mut tree, W, H);
    let h = render_window(&state).unwrap().1;
    assert!(
        h > 0.0,
        "window_to_clip must publish a positive cull height, got {h}"
    );
}

#[test]
fn nested_scroll_areas_bound_the_window_to_their_intersection() {
    use crate::primitives::FixedSize;

    // Editor → inner ScrollArea → a 250px-tall box → outer ScrollArea. The editor
    // is only visible through the 250px inner box, so the window follows the
    // *intersection* of both clips, not the outer 700px viewport.
    let ed = make_editor(true);
    let state = ed.state_handle();
    let mut tree = WidgetTree::new();
    let id = tree.add(ed);
    let inner = tree.add(ScrollArea::from_id(id));
    let boxed = tree.add(FixedSize::new().width(W).height(250.0).child_id(inner));
    tree.add(ScrollArea::from_id(boxed));
    settle(&mut tree, W, H);

    let win = render_window(&state).expect("nested-clipped editor still sets a window");
    assert!(
        win.1 < 250.0 * 3.0,
        "the window height ({}) must follow the 250px inner clip, not the 700px outer \
         viewport",
        win.1
    );
}
