// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a theme override costs the layout and paint walks.
//!
//! An app that themes a whole window installs one override at the window's
//! root (`WidgetTree::set_theme_override`), so every widget sits below it.
//! The walks must not pay for that per widget. This bench lays out and paints
//! the same tree with no override and with one at the root, so the two rows
//! of each group read side by side: the gap between them is what the override
//! costs a pass.
//!
//! The tree is about a thousand widgets, six containers deep, which is the
//! size of an ordinary editor window. Layout alternates between two proposals
//! so every pass lays out every widget; paint invalidates every cached frame
//! first so every pass paints every widget. A third group repaints one widget
//! outside the tree and replays the cache for the rest, the shape of most
//! frames: there the override has nothing to theme and should cost nothing.
//!
//! ```text
//! cargo bench -p teksilo-core --bench theme_resolution
//! cargo bench -p teksilo-core --bench theme_resolution -- --save-baseline before
//! cargo bench -p teksilo-core --bench theme_resolution -- --baseline before
//! ```
//!
//! The pass-count guarantees themselves are unit tests,
//! `an_override_runs_once_per_walk_however_large_its_subtree` and
//! `an_override_over_a_clean_subtree_does_not_run_in_paint`; this bench is the
//! instrument for the time they buy.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_tokens::Color;

/// Splits its bounds into equal columns, one per child.
#[derive(Debug)]
struct Columns;

impl Widget for Columns {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        let width = bounds.width / children.len().max(1) as f32;
        for (i, child) in children.iter_mut().enumerate() {
            child.origin = Point::new(bounds.x + width * i as f32, bounds.y);
            child.size = Size::new(width, bounds.height);
        }
    }
}

/// A leaf that paints in a theme colour, so painting reads the resolved theme
/// the way a real control does.
#[derive(Debug)]
struct Swatch;

impl Widget for Swatch {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        canvas.fill_rect(bounds, ctx.theme.colors.accent);
    }
}

/// `DEPTH` levels of containers with `FANOUT` children each, the last level's
/// children being swatches: 364 containers and 729 swatches, 1093 widgets.
const FANOUT: usize = 3;
const DEPTH: usize = 6;

fn build(tree: &mut WidgetTree, parent: WidgetId, depth: usize) {
    for _ in 0..FANOUT {
        if depth + 1 == DEPTH {
            tree.add_child(parent, Swatch);
        } else {
            let child = tree.add_child(parent, Columns);
            build(tree, child, depth + 1);
        }
    }
}

fn fixture(overridden: bool) -> WidgetTree {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let root = tree.add(Columns);
    build(&mut tree, root, 0);
    if overridden {
        tree.set_theme_override(root, |theme| {
            theme.colors.accent = Color::RED;
        });
    }
    tree.layout(SizeProposal::exact(1600.0, 900.0));
    tree.render();
    tree
}

fn bench_theme_resolution(c: &mut Criterion) {
    let mut layout = c.benchmark_group("theme_resolution/layout");
    for (label, overridden) in [("no_override", false), ("root_override", true)] {
        let mut tree = fixture(overridden);
        let mut wide = false;
        layout.bench_function(BenchmarkId::from_parameter(label), |b| {
            b.iter(|| {
                wide = !wide;
                let width = if wide { 1601.0 } else { 1600.0 };
                tree.layout(black_box(SizeProposal::exact(width, 900.0)));
            });
        });
    }
    layout.finish();

    let mut paint = c.benchmark_group("theme_resolution/paint");
    for (label, overridden) in [("no_override", false), ("root_override", true)] {
        let mut tree = fixture(overridden);
        paint.bench_function(BenchmarkId::from_parameter(label), |b| {
            b.iter(|| {
                tree.invalidate_all_paints();
                black_box(tree.render());
            });
        });
    }
    paint.finish();

    let mut outside = c.benchmark_group("theme_resolution/paint_outside");
    for (label, overridden) in [("no_override", false), ("root_override", true)] {
        let mut tree = fixture(overridden);
        let outsider = tree.add(Swatch);
        tree.layout(SizeProposal::exact(1600.0, 900.0));
        tree.render();
        outside.bench_function(BenchmarkId::from_parameter(label), |b| {
            b.iter(|| {
                tree.mark_needs_paint(outsider);
                black_box(tree.render());
            });
        });
    }
    outside.finish();
}

criterion_group!(benches, bench_theme_resolution);
criterion_main!(benches);
