// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `SceneMinimap` must never paint outside its own bounds, and a tap
//! must land on what it painted.
//!
//! Scene coordinates reach the drawing area through a plain affine map
//! which is **unclamped**: a viewport rect not fully inside
//! `content_bounds` projects to minimap-local coordinates outside the
//! drawing area. The minimap is a self-painting leaf, and the render
//! walker emits a `clips_children` node's `SetClip` *after* that node's
//! own `paint()` (see
//! `teksilo-core/src/widget_tree/rendering_impl.rs`, "Self-transform /
//! plain clipping nodes emit their clip here — after the node's own
//! paint"), so the framework will not clip it. Three mechanisms make it
//! safe, and each has its own tests below:
//!
//! 1. the extent **expands** to contain the items and the live viewport,
//!    so projected geometry lands inside the picture rather than being
//!    clamped (a lie) or cropped (dead feedback);
//! 2. `paint` sets its **own clip**, covering every stroke it emits —
//!    the widget's own border included — as a backstop for the widths
//!    the extent policy says nothing about;
//! 3. the border is stroked on an **inset** rect, so `stroke_rect`'s
//!    edge-centred lines do not straddle the frame.
//!
//! There is no tolerance anywhere in this file: after (2) and (3),
//! *nothing* the minimap draws may exceed its bounds by any amount.
//!
//! These tests assert at the `RenderFrame` level, replaying the draw
//! order the way `teksilo-render`'s `Renderer::render` does: composing
//! the transform stack and intersecting the scissor stack. Anything
//! that survives that replay is geometry the GPU will actually draw —
//! and it is also what the tap tests read their probe points from, so
//! "paint and click agree" is checked against the frame rather than
//! against a hard-coded number.
//!
//! Several cases are run twice: once at the root of the tree and once
//! nested in a `Padding`. At the root `bounds.origin` is `(0, 0)`, which
//! makes `paint`'s translate the identity and hides both the
//! clip-after-translate ordering and the local-`area`-not-`bounds`
//! choice that ordering exists to serve.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{DrawCommand, Point, Rect, RenderFrame, SizeProposal, Transform2D};
use teksilo_core::signal::Signal;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_scene::{RectItem, Scene, SceneMinimap};
use teksilo_tokens::Color;
use teksilo_widgets::Padding;

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
fn visible_decorations(frame: &RenderFrame) -> Vec<(Rect, [f32; 4])> {
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
                    out.push((v, d.color));
                }
            }
            _ => {}
        }
    }
    out
}

/// The geometry half of [`visible_decorations`] — what the three
/// original overflow tests assert on.
fn visible_decoration_rects(frame: &RenderFrame) -> Vec<Rect> {
    visible_decorations(frame)
        .into_iter()
        .map(|(r, _)| r)
        .collect()
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

/// Nothing the minimap paints may leave its bounds — by **any** amount.
///
/// There is deliberately no stroke slack. `paint` wraps everything it
/// emits in one `set_clip(area)`, the widget's own border included, and
/// that border is stroked on an inset rect precisely so that
/// `stroke_rect`'s edge-centred lines do not straddle the frame. The
/// replay above intersects with the scissor exactly as the renderer
/// does, so every surviving rect is a subset of the clip. A tolerance
/// here would only hide the class of bug these tests exist to catch.
fn assert_all_inside(label: &str, bounds: Rect, rects: &[Rect]) {
    let escapees: Vec<&Rect> = rects
        .iter()
        .filter(|r| {
            r.x < bounds.x
                || r.y < bounds.y
                || r.right() > bounds.right()
                || r.bottom() > bounds.bottom()
        })
        .collect();
    assert!(
        escapees.is_empty(),
        "{label}: {} of {} drawn rect(s) fall outside the minimap's bounds \
         {bounds:?}. The minimap paints outside its own frame. \
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

// ---------------------------------------------------------------------------
// The other half of the fix: staying inside the frame must not be achieved by
// clamping the indicator (which lies about where the viewport is) or by
// cropping it away (which leaves the user with no feedback at all). The tests
// above would pass under either; these fail under both.
// ---------------------------------------------------------------------------

/// Lay out at `proposal` instead of `unspecified`, and keep each
/// decoration's colour so a specific piece of the minimap can be
/// picked out of the frame.
fn render_minimap_at(mm: SceneMinimap, proposal: SizeProposal) -> (Rect, Vec<(Rect, [f32; 4])>) {
    let mut tree = WidgetTree::new();
    let id = tree.add(mm);
    tree.layout(proposal);
    let bounds = tree.bounds(id);
    let frame = tree.render();
    (bounds, visible_decorations(&frame))
}

fn render_minimap_coloured(mm: SceneMinimap) -> (Rect, Vec<(Rect, [f32; 4])>) {
    render_minimap_at(mm, SizeProposal::unspecified())
}

fn is_color(actual: [f32; 4], want: Color) -> bool {
    actual
        .iter()
        .zip(want.to_array())
        .all(|(a, b)| (a - b).abs() < 1e-3)
}

/// Every decoration painted in `want`, in screen space.
fn parts_in(drawn: &[(Rect, [f32; 4])], want: Color) -> Vec<Rect> {
    drawn
        .iter()
        .filter(|(_, c)| is_color(*c, want))
        .map(|(r, _)| *r)
        .collect()
}

/// AABB of a set of rects — for a stroked rect, its outer edge.
fn union_of(rects: &[Rect]) -> Rect {
    assert!(!rects.is_empty(), "union_of needs at least one rect");
    let mut acc = rects[0];
    for r in &rects[1..] {
        let x = acc.x.min(r.x);
        let y = acc.y.min(r.y);
        let right = acc.right().max(r.right());
        let bottom = acc.bottom().max(r.bottom());
        acc = Rect::new(x, y, right - x, bottom - y);
    }
    acc
}

/// A minimap whose only coloured geometry is its viewport indicator:
/// no border, no content outline, no items.
fn indicator_only(content: Rect, viewport: Rect, indicator: Color) -> SceneMinimap {
    SceneMinimap::new(content, Signal::new(viewport))
        .viewport_color(indicator)
        .border(None)
        .size(200.0, 200.0)
}

/// Clipping alone would "fix" the overflow by making the indicator
/// vanish the moment the viewport leaves the content — the same dead
/// feedback as before, reached a different way. It must still be drawn.
#[test]
fn the_indicator_is_still_drawn_when_the_viewport_is_off_the_content() {
    let indicator = Color::new(0.1, 0.4, 0.9, 1.0);
    let mm = indicator_only(
        Rect::new(0.0, 0.0, 1000.0, 1000.0),
        Rect::new(3000.0, 3000.0, 200.0, 200.0),
        indicator,
    );

    let (bounds, drawn) = render_minimap_coloured(mm);
    let parts = parts_in(&drawn, indicator);
    assert_eq!(
        parts.len(),
        4,
        "a stroked rect is four edge lines; the indicator must survive the clip, \
         got {} piece(s) — clipping is not a fix on its own",
        parts.len()
    );
    assert_all_inside("indicator off content", bounds, &parts);
}

/// Clamping the indicator to the frame would keep it on screen while
/// lying about both where the viewport is and how big it is. After the
/// extent expansion the indicator is 200/3200 of the picture and sits
/// in the far corner, which is the truth.
#[test]
fn the_indicator_keeps_its_position_and_size_when_the_viewport_is_off_the_content() {
    let indicator = Color::new(0.1, 0.4, 0.9, 1.0);
    let mm = indicator_only(
        Rect::new(0.0, 0.0, 1000.0, 1000.0),
        Rect::new(3000.0, 3000.0, 200.0, 200.0),
        indicator,
    );

    let (bounds, drawn) = render_minimap_coloured(mm);
    let box_ = union_of(&parts_in(&drawn, indicator));

    assert!(
        (box_.width - box_.height).abs() < 0.5,
        "a square viewport must stay square: {box_:?}"
    );
    // extent = content ∪ viewport = 3200 wide, viewport = 200 wide, so the
    // indicator is ~6.25 % of the picture plus its 2 px stroke. A clamped
    // indicator would keep the pre-expansion 20 %, a collapsed one ~0 %.
    let fraction = box_.width / bounds.width;
    assert!(
        (0.05..0.10).contains(&fraction),
        "indicator should be ~6-7 % of the minimap, got {:.1} % ({box_:?})",
        fraction * 100.0
    );
    let centre = box_.center();
    let mid = bounds.center();
    assert!(
        centre.x > mid.x && centre.y > mid.y,
        "the viewport is past the far corner of the content, so the indicator \
         belongs in the bottom-trailing quadrant: {centre:?} vs {mid:?}"
    );
    assert_all_inside(
        "indicator off content",
        bounds,
        &parts_in(&drawn, indicator),
    );
}

/// A minimap whose aspect ratio differs from the scene's must letterbox,
/// not stretch: a square item has to read as a square.
#[test]
fn a_non_square_minimap_letterboxes_instead_of_stretching() {
    let item = Color::new(0.9, 0.1, 0.1, 1.0);
    let mm = SceneMinimap::new(
        Rect::new(0.0, 0.0, 1000.0, 1000.0),
        Signal::new(Rect::new(100.0, 100.0, 400.0, 400.0)),
    )
    .items(vec![(Rect::new(100.0, 100.0, 200.0, 200.0), item)])
    .border(None)
    .size(300.0, 100.0);

    let (bounds, drawn) = render_minimap_coloured(mm);
    assert_eq!((bounds.width, bounds.height), (300.0, 100.0));
    let thumbs = parts_in(&drawn, item);
    assert_eq!(thumbs.len(), 1, "one item, one filled thumbnail");
    assert!(
        (thumbs[0].width - thumbs[0].height).abs() < 0.5,
        "a square item in a 3:1 minimap must stay square, got {:?}",
        thumbs[0]
    );
    assert_all_inside("letterboxed", bounds, &visible_rects(&drawn));
}

/// An empty scene: a zero-extent `content_bounds`, and — before the view
/// transform is invertible — a zero-extent viewport too. The projection
/// must stay finite and inside the frame rather than dividing by zero.
#[test]
fn an_empty_scene_paints_inside_its_frame() {
    let indicator = Color::new(0.1, 0.4, 0.9, 1.0);
    for (label, content, viewport) in [
        ("degenerate everything", Rect::ZERO, Rect::ZERO),
        (
            "no content, real window",
            Rect::ZERO,
            Rect::new(0.0, 0.0, 800.0, 600.0),
        ),
    ] {
        let mm = indicator_only(content, viewport, indicator);
        let (bounds, drawn) = render_minimap_coloured(mm);
        let rects = visible_rects(&drawn);
        assert!(
            !rects.is_empty(),
            "{label}: the minimap must paint something"
        );
        for r in &rects {
            assert!(
                r.x.is_finite() && r.y.is_finite() && r.width.is_finite() && r.height.is_finite(),
                "{label}: a degenerate extent must not produce a non-finite rect: {r:?}"
            );
        }
        assert_eq!(
            parts_in(&drawn, indicator).len(),
            4,
            "{label}: the indicator is still drawn, collapsed to a dot"
        );
        assert_all_inside(label, bounds, &rects);
    }
}

/// A rotated camera delivers `viewport_in_scene` as the **AABB** of the
/// rotated viewport (`Transform2D::apply_rect` returns a bounding box),
/// which is both larger than the viewport and off-origin. The minimap
/// must survive that the same way it survives a zoom-out.
#[test]
fn a_rotated_viewport_aabb_stays_inside_the_minimap() {
    let indicator = Color::new(0.1, 0.4, 0.9, 1.0);
    let screen = Rect::new(0.0, 0.0, 800.0, 600.0);
    // What the inverse view transform hands back for a 30°-rotated camera.
    let rotated = Transform2D::rotate(std::f32::consts::FRAC_PI_6).apply_rect(screen);
    assert!(
        rotated.width > screen.width,
        "a rotated AABB is wider than the viewport it bounds"
    );

    let mm = indicator_only(Rect::new(0.0, 0.0, 300.0, 300.0), rotated, indicator);
    let (bounds, drawn) = render_minimap_coloured(mm);
    assert_eq!(parts_in(&drawn, indicator).len(), 4);
    assert_all_inside("rotated viewport aabb", bounds, &visible_rects(&drawn));
}

/// The thumbnail feed: `Scene::item_thumbnails` reports **both** tiers,
/// and an app that snapshotted `content_bounds` earlier (or picked one by
/// hand) can easily hold an extent that no longer contains them. Neither
/// tier may paint outside the frame, and neither may be cropped away.
#[test]
fn thumbnails_from_a_real_scene_stay_inside_a_stale_content_bounds() {
    let mut scene = Scene::new();
    scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(Color::RED),
        Point::new(10.0, 10.0),
    );
    // A heavyweight card far outside the extent the app remembered.
    scene.add_widget(
        teksilo_widgets::RectWidget::new(),
        Rect::new(4000.0, 20.0, 120.0, 60.0),
    );
    let thumbs = scene.item_thumbnails();
    assert_eq!(thumbs.len(), 2, "both tiers contribute a thumbnail");

    let stale = Rect::new(0.0, 0.0, 100.0, 100.0);
    let mm = SceneMinimap::new(stale, Signal::new(Rect::new(0.0, 0.0, 100.0, 100.0)))
        .items(thumbs.clone())
        .border(None)
        .size(200.0, 200.0);

    let (bounds, drawn) = render_minimap_coloured(mm);
    assert_all_inside("stale content bounds", bounds, &visible_rects(&drawn));
    for (rect, color) in &thumbs {
        assert_eq!(
            parts_in(&drawn, *color).len(),
            1,
            "the thumbnail for {rect:?} must still be visible — expanding the \
             extent is the fix, cropping it away is not"
        );
    }
}

/// The clip on its own, with the extent expansion held harmless.
///
/// Expanding the extent keeps *projected rects* inside the frame, but it
/// says nothing about the **stroke width** an app hangs on one of them:
/// `content_outline` takes a caller-supplied width, and `stroke_rect`
/// centres each edge line on the boundary, so half of a wide outline
/// falls outside the picture by construction. Only the clip stops that,
/// which makes this the test that reddens if `set_clip` is removed —
/// every other case here is already inside once the extent has grown.
///
/// (The widget's own 1 px *border* is deliberately drawn outside the
/// clip: it marks the widget's edge and is meant to straddle it. The
/// outline is projected scene content, and content does not escape.)
#[test]
fn a_thick_content_outline_cannot_bleed_past_the_frame() {
    let outline = Color::new(0.0, 0.6, 0.3, 1.0);
    // Viewport inside the content, so the extent IS `content_bounds` and
    // the outline lands exactly on the edge of the drawing area — where
    // half of its 12 px stroke would hang outside the widget.
    let mm = SceneMinimap::new(
        Rect::new(0.0, 0.0, 1000.0, 1000.0),
        Signal::new(Rect::new(250.0, 250.0, 500.0, 500.0)),
    )
    .content_outline(Some((outline, 12.0)))
    .border(None)
    .size(200.0, 200.0);

    let (bounds, drawn) = render_minimap_coloured(mm);
    let parts = parts_in(&drawn, outline);
    assert_eq!(parts.len(), 4, "the outline is still drawn, not dropped");
    assert_all_inside("thick content outline", bounds, &parts);
}

/// A parent can hand the minimap less than it asked for. Paint must
/// project against the area it was actually given, not the size it
/// declared, or the picture overflows the smaller rect.
#[test]
fn a_minimap_given_less_than_it_asked_for_paints_inside_its_bounds() {
    let indicator = Color::new(0.1, 0.4, 0.9, 1.0);
    let mm = indicator_only(
        Rect::new(0.0, 0.0, 1000.0, 1000.0),
        Rect::new(200.0, 200.0, 400.0, 400.0),
        indicator,
    );

    let (bounds, drawn) = render_minimap_at(mm, SizeProposal::exact(120.0, 60.0));
    assert_eq!(
        (bounds.width, bounds.height),
        (120.0, 60.0),
        "the minimap must accept the smaller area"
    );
    assert_eq!(parts_in(&drawn, indicator).len(), 4);
    assert_all_inside("smaller than declared", bounds, &visible_rects(&drawn));
}

/// Click-to-recentre must invert **what the minimap actually painted**.
///
/// The old shape of this test hard-coded the expected scene point, which
/// made it vacuous: it kept passing while paint projected through one
/// extent and the tap closure through another, an 1100-unit
/// disagreement. So it asserts nothing about agreement at all.
///
/// This one derives the *probe* from the rendered frame and the
/// *expectation* from the scene inputs, with nothing in between:
///
/// - find the item's thumbnail in the frame, tap its centre on screen,
///   and demand back the item's own scene centre;
/// - find the viewport indicator in the frame, tap its centre, and
///   demand back the viewport's scene centre.
///
/// Neither number is copied from the projection. If paint and tap ever
/// disagree about the extent, the drawing area, or the widget's origin,
/// the two probes land on different scene points than the geometry they
/// were read from, and this reddens.
///
/// It is run twice: at the tree root (origin `(0, 0)`) and nested inside
/// a `Padding`, where screen and widget-local coordinates differ.
fn tap_lands_on_what_was_painted(nest: Option<f32>) {
    let item_colour = Color::new(0.9, 0.1, 0.1, 1.0);
    let indicator = Color::new(0.1, 0.4, 0.9, 1.0);
    let item_scene = Rect::new(100.0, 100.0, 200.0, 200.0);
    // Off the content, so the extent has to expand — the case the old
    // assertion was written around.
    let viewport_scene = Rect::new(3000.0, 3000.0, 200.0, 200.0);

    let got: Rc<Cell<Option<Point>>> = Rc::new(Cell::new(None));
    let sink = Rc::clone(&got);
    let mm = SceneMinimap::new(
        Rect::new(0.0, 0.0, 1000.0, 1000.0),
        Signal::new(viewport_scene),
    )
    .items(vec![(item_scene, item_colour)])
    .viewport_color(indicator)
    .border(None)
    .size(200.0, 200.0)
    .on_click(move |p, _| sink.set(Some(p)));

    let mut tree = WidgetTree::new();
    let id = tree.add(mm);
    if let Some(inset) = nest {
        let _root = tree.add(Padding::uniform(inset).child(id));
    }
    tree.layout(SizeProposal::unspecified());
    // Paint records the projection the tap must agree with.
    let drawn = visible_decorations(&tree.render());
    let bounds = tree.bounds(id);
    assert_eq!(
        (bounds.x, bounds.y),
        (nest.unwrap_or(0.0), nest.unwrap_or(0.0)),
        "the nesting must actually offset the minimap"
    );

    let thumbs = parts_in(&drawn, item_colour);
    assert_eq!(thumbs.len(), 1, "one item, one filled thumbnail");
    let probes = [
        ("item thumbnail", thumbs[0].center(), item_scene.center()),
        (
            "viewport indicator",
            union_of(&parts_in(&drawn, indicator)).center(),
            viewport_scene.center(),
        ),
    ];

    for (what, on_screen, want) in probes {
        got.set(None);
        tree.tap_with(teksilo_tokens::PointerKind::Mouse, on_screen);
        let hit = got
            .get()
            .unwrap_or_else(|| panic!("{what}: on_click must fire for a tap on the minimap"));
        assert!(
            (hit.x - want.x).abs() < 1.0 && (hit.y - want.y).abs() < 1.0,
            "tapping the centre of the painted {what} (screen {on_screen:?}) must \
             return that geometry's scene centre {want:?}, got {hit:?} — paint and \
             tap are projecting through different transforms"
        );
    }
}

#[test]
fn a_tap_maps_back_to_the_scene_point_the_minimap_painted() {
    tap_lands_on_what_was_painted(None);
}

#[test]
fn a_tap_maps_back_to_the_painted_point_when_the_minimap_is_offset() {
    tap_lands_on_what_was_painted(Some(60.0));
}

/// A tap must project against the area the minimap was *given*, not the
/// size it declared. (Complements the pair above, which lay out at the
/// declared size; here the parent hands over a quarter of it.)
#[test]
fn a_tap_projects_against_the_area_the_minimap_was_given() {
    let indicator = Color::new(0.1, 0.4, 0.9, 1.0);
    let viewport_scene = Rect::new(3000.0, 3000.0, 200.0, 200.0);
    let got: Rc<Cell<Option<Point>>> = Rc::new(Cell::new(None));
    let sink = Rc::clone(&got);

    // Declared 200×200 but laid out at 100×100: a tap projected against
    // the declared size would land somewhere else entirely.
    let mm = SceneMinimap::new(
        Rect::new(0.0, 0.0, 1000.0, 1000.0),
        Signal::new(viewport_scene),
    )
    .viewport_color(indicator)
    .border(None)
    .size(200.0, 200.0)
    .on_click(move |p, _| sink.set(Some(p)));

    let mut tree = WidgetTree::new();
    let id = tree.add(mm);
    tree.layout(SizeProposal::exact(100.0, 100.0));
    let drawn = visible_decorations(&tree.render());
    let bounds = tree.bounds(id);
    assert_eq!((bounds.width, bounds.height), (100.0, 100.0));

    let on_screen = union_of(&parts_in(&drawn, indicator)).center();
    tree.tap_with(teksilo_tokens::PointerKind::Mouse, on_screen);
    let hit = got
        .get()
        .expect("on_click must fire for a tap on the minimap");
    let want = viewport_scene.center();
    assert!(
        (hit.x - want.x).abs() < 1.0 && (hit.y - want.y).abs() < 1.0,
        "tapping the painted indicator in a shrunken minimap must still return \
         the viewport's scene centre {want:?}, got {hit:?}"
    );
}

/// A tap that beats the first paint has no painted frame to invert, so
/// `build` seeds the projection from the effective extent at the
/// declared size. Drop that seed and the tap falls back to whatever
/// `new` happened to store, which knows neither `.items(..)` nor the
/// live viewport.
#[test]
fn a_tap_before_the_first_paint_uses_the_seeded_projection() {
    let content = Rect::new(0.0, 0.0, 1000.0, 1000.0);
    let viewport = Rect::new(3000.0, 3000.0, 200.0, 200.0);
    let got: Rc<Cell<Option<Point>>> = Rc::new(Cell::new(None));
    let sink = Rc::clone(&got);

    let mm = SceneMinimap::new(content, Signal::new(viewport))
        .size(400.0, 400.0)
        .on_click(move |p, _| sink.set(Some(p)));

    let mut tree = WidgetTree::new();
    let id = tree.add(mm);
    tree.layout(SizeProposal::unspecified());
    // Deliberately no `tree.render()`.
    let bounds = tree.bounds(id);
    tree.tap_with(teksilo_tokens::PointerKind::Mouse, bounds.center());

    let hit = got
        .get()
        .expect("on_click must fire for a tap on the minimap");
    // effective extent = content ∪ viewport = (0, 0, 3200, 3200), whose
    // centre is what the centre of the drawing area shows.
    assert!(
        (hit.x - 1600.0).abs() < 1.0 && (hit.y - 1600.0).abs() < 1.0,
        "a pre-paint tap in the middle must map to the middle of the effective \
         extent, got {hit:?}"
    );
}

// ---------------------------------------------------------------------------
// The widget's own border. Everything above is *projected scene* geometry,
// which the extent expansion already keeps inside the frame; the border is
// drawn in widget-local coordinates at a width the caller picks, and
// `stroke_rect` centres its edge lines on the rect it is handed. Stroking the
// bounds directly therefore hung half the width over the neighbours — 12 px a
// side for a 24 px border — from a public builder, with no clamp and no
// warning. It is now stroked on the inset rect, inside the clip.
// ---------------------------------------------------------------------------

/// `width` px of border, lying wholly inside `bounds` and hugging it.
///
/// The thickness check is what catches a border stroked on the bounds
/// rather than on the inset rect: the clip then shaves each edge to
/// `width / 2`, which is contained but is no longer the border the
/// caller asked for.
fn assert_border_band(label: &str, bounds: Rect, parts: &[Rect], width: f32) {
    assert_eq!(parts.len(), 4, "{label}: the border is four visible edges");
    assert_all_inside(label, bounds, parts);
    for r in parts {
        let thickness = r.width.min(r.height);
        assert!(
            (thickness - width).abs() < 0.01,
            "{label}: each border edge must be the full {width} px the caller \
             asked for, got {thickness} ({r:?})"
        );
    }
    // The band hugs the frame: its outer edge IS the widget's edge.
    let outer = union_of(parts);
    assert!(
        (outer.x - bounds.x).abs() < 0.01
            && (outer.y - bounds.y).abs() < 0.01
            && (outer.right() - bounds.right()).abs() < 0.01
            && (outer.bottom() - bounds.bottom()).abs() < 0.01,
        "{label}: the border must sit on the inside of the frame, not float \
         within it: {outer:?} vs {bounds:?}"
    );
}

/// A border thick enough to matter must lie wholly inside the widget it
/// frames, and must still be four full-width edges.
#[test]
fn a_thick_border_cannot_bleed_past_the_frame() {
    let border = Color::new(0.0, 0.2, 0.8, 1.0);
    let width = 24.0;
    let mm = SceneMinimap::new(
        Rect::new(0.0, 0.0, 1000.0, 1000.0),
        Signal::new(Rect::new(250.0, 250.0, 500.0, 500.0)),
    )
    .border(Some((border, width)))
    .size(200.0, 200.0);

    let (bounds, drawn) = render_minimap_coloured(mm);
    assert_border_band("thick border", bounds, &parts_in(&drawn, border), width);
}

/// A width that is not a drawable number emits nothing rather than
/// putting a `NaN` or inside-out rect in the frame. Both stroke widths
/// the minimap takes are caller-supplied and land straight in a
/// decoration rect; every *rect* input is screened, and these were the
/// two numbers that screening missed.
#[test]
fn a_non_finite_or_negative_stroke_width_draws_nothing() {
    let border = Color::new(0.0, 0.2, 0.8, 1.0);
    let outline = Color::new(0.0, 0.6, 0.3, 1.0);
    for width in [f32::NAN, f32::INFINITY, -8.0, 0.0] {
        let mm = SceneMinimap::new(
            Rect::new(0.0, 0.0, 1000.0, 1000.0),
            Signal::new(Rect::new(250.0, 250.0, 500.0, 500.0)),
        )
        .border(Some((border, width)))
        .content_outline(Some((outline, width)))
        .size(200.0, 200.0);

        let (bounds, drawn) = render_minimap_coloured(mm);
        assert!(
            parts_in(&drawn, border).is_empty() && parts_in(&drawn, outline).is_empty(),
            "width {width} must draw no stroke at all"
        );
        for r in visible_rects(&drawn) {
            assert!(
                r.x.is_finite() && r.y.is_finite() && r.width.is_finite() && r.height.is_finite(),
                "width {width} must not put a non-finite rect in the frame: {r:?}"
            );
        }
        assert_all_inside("bad stroke width", bounds, &visible_rects(&drawn));
    }
}

/// The pathological width: a border wider than the widget. The inset
/// collapses, and only the clip keeps the strokes off the neighbours.
#[test]
fn a_border_wider_than_the_widget_is_still_contained() {
    let border = Color::new(0.0, 0.2, 0.8, 1.0);
    let mm = SceneMinimap::new(
        Rect::new(0.0, 0.0, 1000.0, 1000.0),
        Signal::new(Rect::new(250.0, 250.0, 500.0, 500.0)),
    )
    .border(Some((border, 500.0)))
    .size(200.0, 200.0);

    let (bounds, drawn) = render_minimap_coloured(mm);
    assert_all_inside("absurd border", bounds, &visible_rects(&drawn));
}

// ---------------------------------------------------------------------------
// Non-zero origin. `paint` translates the canvas to `bounds.origin` and then
// clips to the widget-*local* area; at the root of a tree that translation is
// the identity, so the ordering — and the choice of `area` over `bounds` — is
// unobservable. Every case below places the minimap inside a `Padding` so both
// are load-bearing.
// ---------------------------------------------------------------------------

/// Build `mm` inside a `Padding`, so its bounds' origin is not `(0, 0)`.
fn render_nested(mm: SceneMinimap, inset: f32) -> (Rect, Vec<(Rect, [f32; 4])>) {
    let mut tree = WidgetTree::new();
    let mm_id = tree.add(mm);
    let _root = tree.add(Padding::uniform(inset).child(mm_id));
    tree.layout(SizeProposal::unspecified());
    let bounds = tree.bounds(mm_id);
    let frame = tree.render();
    (bounds, visible_decorations(&frame))
}

/// The clip must land on the widget wherever the parent put it. Set it
/// before the translate, or hand it `bounds` instead of the local
/// `area`, and it lands 60 px off — cropping two of the outline's edges
/// away entirely.
#[test]
fn an_offset_minimap_clips_to_itself_and_not_to_the_window_origin() {
    let outline = Color::new(0.0, 0.6, 0.3, 1.0);
    let mm = SceneMinimap::new(
        Rect::new(0.0, 0.0, 1000.0, 1000.0),
        Signal::new(Rect::new(250.0, 250.0, 500.0, 500.0)),
    )
    .content_outline(Some((outline, 12.0)))
    .border(None)
    .size(200.0, 200.0);

    let (bounds, drawn) = render_nested(mm, 60.0);
    assert_eq!(
        (bounds.x, bounds.y, bounds.width, bounds.height),
        (60.0, 60.0, 200.0, 200.0),
        "the padding must offset the minimap"
    );
    let parts = parts_in(&drawn, outline);
    assert_eq!(
        parts.len(),
        4,
        "all four outline edges survive a clip aligned with the widget; a clip \
         left at the window origin crops two of them away"
    );
    assert_all_inside("offset, thick outline", bounds, &parts);
    assert_all_inside("offset, everything", bounds, &visible_rects(&drawn));
}

/// The border, offset. Same shape as the at-origin case, run where the
/// translate is not the identity.
#[test]
fn an_offset_minimaps_thick_border_stays_inside_its_frame() {
    let border = Color::new(0.0, 0.2, 0.8, 1.0);
    let mm = SceneMinimap::new(
        Rect::new(0.0, 0.0, 1000.0, 1000.0),
        Signal::new(Rect::new(3000.0, 3000.0, 200.0, 200.0)),
    )
    .border(Some((border, 24.0)))
    .size(200.0, 200.0);

    let (bounds, drawn) = render_nested(mm, 60.0);
    assert_eq!((bounds.x, bounds.y), (60.0, 60.0));
    assert_border_band(
        "offset, thick border",
        bounds,
        &parts_in(&drawn, border),
        24.0,
    );
}

/// The off-content indicator, offset: the picture must move with the
/// widget, not stay pinned at the window's top-left.
#[test]
fn an_offset_minimap_paints_its_picture_at_its_own_origin() {
    let indicator = Color::new(0.1, 0.4, 0.9, 1.0);
    let mm = indicator_only(
        Rect::new(0.0, 0.0, 1000.0, 1000.0),
        Rect::new(3000.0, 3000.0, 200.0, 200.0),
        indicator,
    );

    let (bounds, drawn) = render_nested(mm, 60.0);
    let parts = parts_in(&drawn, indicator);
    assert_eq!(parts.len(), 4, "the indicator survives the clip");
    let box_ = union_of(&parts);
    assert!(
        box_.x >= bounds.x && box_.y >= bounds.y,
        "the picture must be drawn in the widget's frame, not the window's: \
         {box_:?} vs {bounds:?}"
    );
    assert_all_inside("offset indicator", bounds, &visible_rects(&drawn));
}

fn visible_rects(drawn: &[(Rect, [f32; 4])]) -> Vec<Rect> {
    drawn.iter().map(|(r, _)| *r).collect()
}
