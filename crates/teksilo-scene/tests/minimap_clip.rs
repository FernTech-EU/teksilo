// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `SceneMinimap` must never paint outside its own bounds.
//!
//! The minimap projects scene coordinates onto its drawing area with a
//! plain affine map (`scene_to_minimap`), which is **unclamped**: a
//! viewport rect that is not fully inside `content_bounds` projects to
//! minimap-local coordinates outside the drawing area. Because the
//! minimap is a self-painting leaf that emits no clip of its own — and
//! because the render walker emits a `clips_children` node's `SetClip`
//! *after* that node's own `paint()` (see
//! `teksilo-core/src/widget_tree/rendering_impl.rs`, "Self-transform /
//! plain clipping nodes emit their clip here — after the node's own
//! paint") — nothing stops that geometry from reaching the screen.
//!
//! These tests assert at the `RenderFrame` level, replaying the draw
//! order the way `teksilo-render`'s `Renderer::render` does: composing
//! the transform stack and intersecting the scissor stack. Anything
//! that survives that replay is geometry the GPU will actually draw.

use teksilo_canvas::{DrawCommand, Rect, RenderFrame, SizeProposal, Transform2D};
use teksilo_core::signal::Signal;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_scene::SceneMinimap;
use teksilo_tokens::Color;

/// Slack for hairline strokes: `stroke_rect` centres each edge line on
/// the rect boundary, so a 1 px border legitimately covers 0.5 px
/// outside `bounds`, and the 2 px viewport stroke 1 px. Anything beyond
/// this is real overflow, not stroke centring.
const STROKE_SLACK: f32 = 1.5;

fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = a.right().min(b.right());
    let y1 = a.bottom().min(b.bottom());
    if x1 > x0 && y1 > y0 {
        Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
    } else {
        None
    }
}

/// Replay `frame`'s draw order exactly as `Renderer::render` does and
/// return every decoration rect that survives to the screen, in screen
/// space, already reduced by the active scissor.
///
/// - `PushTransform(t)` composes `t` onto the stack top and pushes.
/// - `SetTransform(t)` composes `t` with the stack top without pushing
///   (this is what `Canvas::translate` / `save` / `restore` emit).
/// - `SetClip(rect)` transforms `rect` by the *current* transform, then
///   intersects with the scissor-stack top; `ClearClip` pops.
fn visible_decoration_rects(frame: &RenderFrame) -> Vec<Rect> {
    let mut stack: Vec<Transform2D> = vec![Transform2D::identity()];
    let mut current = Transform2D::identity();
    let mut clips: Vec<Rect> = Vec::new();
    let mut out = Vec::new();

    for cmd in &frame.draw_order {
        match cmd {
            DrawCommand::PushTransform(t) => {
                let top = t.then(stack.last().expect("stack never empty"));
                stack.push(top);
                current = top;
            }
            DrawCommand::PopTransform => {
                stack.pop();
                current = *stack.last().expect("stack never empty");
            }
            DrawCommand::SetTransform(t) => {
                current = t.then(stack.last().expect("stack never empty"));
            }
            DrawCommand::SetClip(rect) => {
                let screen = current.apply_rect(*rect);
                let effective = match clips.last() {
                    Some(prev) => intersect(screen, *prev).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
                    None => screen,
                };
                clips.push(effective);
            }
            DrawCommand::ClearClip => {
                clips.pop();
            }
            DrawCommand::Decoration(i) => {
                let d = &frame.decorations[*i];
                let raw = Rect::new(d.rect[0], d.rect[1], d.rect[2], d.rect[3]);
                let screen = current.apply_rect(raw);
                let visible = match clips.last() {
                    Some(clip) => intersect(screen, *clip),
                    None => Some(screen),
                };
                if let Some(v) = visible {
                    out.push(v);
                }
            }
            _ => {}
        }
    }
    out
}

/// Build a tree with a single minimap at the root, lay it out, render,
/// and return `(minimap_bounds, visible_decoration_rects)`.
fn render_minimap(mm: SceneMinimap) -> (Rect, Vec<Rect>) {
    let mut tree = WidgetTree::new();
    let id = tree.add(mm);
    // `unspecified` lets the root honour `layout_response` (an *exact*
    // proposal force-sizes the root to the window, which would hide the
    // minimap's own declared size behind an 800x600 drawing area).
    tree.layout(SizeProposal::unspecified());
    let bounds = tree.bounds(id);
    let frame = tree.render();
    (bounds, visible_decoration_rects(&frame))
}

fn assert_all_inside(label: &str, bounds: Rect, rects: &[Rect]) {
    let allowed = Rect::new(
        bounds.x - STROKE_SLACK,
        bounds.y - STROKE_SLACK,
        bounds.width + 2.0 * STROKE_SLACK,
        bounds.height + 2.0 * STROKE_SLACK,
    );
    let escapees: Vec<&Rect> = rects
        .iter()
        .filter(|r| {
            r.x < allowed.x
                || r.y < allowed.y
                || r.right() > allowed.right()
                || r.bottom() > allowed.bottom()
        })
        .collect();
    assert!(
        escapees.is_empty(),
        "{label}: {} of {} drawn rect(s) fall outside the minimap's bounds \
         {bounds:?} (slack {STROKE_SLACK}). The minimap paints unclipped. \
         Offenders: {escapees:#?}",
        escapees.len(),
        rects.len(),
    );
}

/// Zoomed IN and panned past the content: the viewport is a small scene
/// rect a long way outside `content_bounds`. The indicator projects to
/// minimap-local coordinates far outside the drawing area and is drawn
/// there, over whatever sibling widget happens to be underneath.
#[test]
fn viewport_indicator_stays_inside_the_minimap_when_panned_past_the_content() {
    let content = Rect::new(0.0, 0.0, 1000.0, 1000.0);
    // A zoomed-in viewport (small in scene units) parked at scene
    // (3000, 3000) — three content-widths past the far corner. There is
    // no default pan clamp (`Scene::current_pan_bounds()` is `None`), so
    // a user can always reach this state.
    let viewport = Signal::new(Rect::new(3000.0, 3000.0, 200.0, 200.0));
    let mm = SceneMinimap::new(content, viewport).size(200.0, 200.0);

    let (bounds, rects) = render_minimap(mm);
    assert_eq!(
        (bounds.width, bounds.height),
        (200.0, 200.0),
        "the minimap must be laid out at its declared size"
    );
    assert_all_inside("panned past content", bounds, &rects);
}

/// Zoomed OUT past the content: the viewport in scene coordinates is
/// larger than `content_bounds`, so the indicator projects to a rect
/// larger than the whole minimap and its four edges land outside.
#[test]
fn viewport_indicator_stays_inside_the_minimap_when_zoomed_out_past_the_content() {
    let content = Rect::new(0.0, 0.0, 1000.0, 1000.0);
    let viewport = Signal::new(Rect::new(-1000.0, -1000.0, 3000.0, 3000.0));
    let mm = SceneMinimap::new(content, viewport).size(200.0, 200.0);

    let (bounds, rects) = render_minimap(mm);
    assert_all_inside("zoomed out past content", bounds, &rects);
}

/// The same unclamped projection is applied to item thumbnails. An item
/// outside the supplied `content_bounds` — routine when the app passes a
/// hand-picked extent rather than `scene_content_bounds()`, or when
/// items move after the thumbnails were snapshotted — paints outside the
/// minimap too.
#[test]
fn item_thumbnails_stay_inside_the_minimap() {
    let content = Rect::new(0.0, 0.0, 1000.0, 1000.0);
    let viewport = Signal::new(Rect::new(0.0, 0.0, 500.0, 500.0));
    let mm = SceneMinimap::new(content, viewport)
        .items(vec![(Rect::new(2000.0, 2000.0, 100.0, 100.0), Color::RED)])
        .size(200.0, 200.0);

    let (bounds, rects) = render_minimap(mm);
    assert_all_inside("item outside content_bounds", bounds, &rects);
}

/// Control: a viewport fully inside the content projects inside the
/// minimap. This must PASS both before and after the fix — it pins that
/// the replay harness itself is not simply reporting everything as
/// out-of-bounds.
#[test]
fn viewport_indicator_inside_the_content_is_already_in_bounds() {
    let content = Rect::new(0.0, 0.0, 1000.0, 1000.0);
    let viewport = Signal::new(Rect::new(250.0, 250.0, 500.0, 500.0));
    let mm = SceneMinimap::new(content, viewport).size(200.0, 200.0);

    let (bounds, rects) = render_minimap(mm);
    assert!(!rects.is_empty(), "the minimap must paint something");
    assert_all_inside("viewport inside content", bounds, &rects);
}
