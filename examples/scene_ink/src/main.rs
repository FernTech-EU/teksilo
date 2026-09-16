// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `scene-ink` — freehand ink on a `teksilo-scene` page, between two notes.
//!
//! The four framework pieces an ink tool cannot work around, each exercised
//! here so the claim is runnable and not only readable:
//!
//! 1. **Every sample.** The stroke is fed from `on_pointer_event`, which fans
//!    out over [`EventContext::coalesced`] — every position the platform
//!    delivered, each with its own pressure — and then over the packet's own.
//!    Deliberately **not** `on_drag`: the drag recognizer returns `Pending` for
//!    every move inside the slop and then reports the *press* position, so the
//!    first few millimetres of a stroke, where its taper lives, never arrive.
//! 2. **A wet surface.** [`WetLayer`] is a node of its own, so a sample
//!    repaints the stroke in flight and nothing else. The view's `paint` —
//!    which runs the whole `Under` band — does not re-run.
//! 3. **Between two notes.** A finished stroke is added as a lightweight
//!    [`PathItem`] in [`SceneLayer::Interleaved`] at the ink layer's `z`, so it
//!    sits above the note at `z = 0` and below the note at `z = 20`. The two
//!    notes are **heavyweight cards** — real widgets in the arena — and that is
//!    load-bearing, not incidental: `Interleaved` slots a lightweight item into
//!    the arena's child walk, so it orders ink against *cards*. It does not
//!    order it against the `Under` band, which paints whole inside
//!    `SceneView::paint` before the first child. Put an interleaved item in a
//!    scene made only of lightweight items and it is above everything, whatever
//!    its `z`.
//! 4. **No cliff.** The wet stroke is drawn as immutable 32-point chunks plus a
//!    short live tail, so the mask cache hits on everything but the tail — an
//!    order of magnitude under re-filling the whole growing outline, which is
//!    what the chunking is for. `Path::stamp` is a second, smaller win on top
//!    (it is the per-chunk lookups, not the tail's raster, that it makes O(1));
//!    see `docs/ink.md` for both measurements and which test gates which.
//!
//! The **representation** is the app's, and this is the shape to copy:
//! perfect-freehand's closed outline polygon (a per-point radius offset
//! perpendicular to the tangent — variable width is not a constant-width
//! stroked path, which is exactly why a `fill_path` and never a `stroke_path`),
//! with PencilKit's storage model (keep the sampled points, regenerate the
//! outline) and its point-erase-as-a-mask trick. The framework ships none of
//! it: brush feel is policy.
//!
//! Run with: `cargo run -p scene-ink`
//!
//! - Draw with the mouse or a stylus anywhere on the page.
//! - `Backspace` removes the last stroke.
//! - Pan with the middle button or a trackpad; `Ctrl` + wheel zooms.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo::canvas::{Canvas, FillRule, Path, Point, Rect, Size};
use teksilo::prelude::*;
use teksilo::tokens::{Alignment, CornerRadius, HAlignment};
use teksilo::widgets::{Expand, Padding, RectWidget, TextWidget, VStack, ZStack};
use teksilo_scene::{
    ItemId, PathItem, SceneItemPaintContext, SceneLayer, SceneModel, SceneView, WetLayer,
};

// ---------------------------------------------------------------------------
// The representation — app-layer, recommended, not shipped by the framework
// ---------------------------------------------------------------------------

/// One sampled control point. PencilKit's storage model: keep the samples and
/// regenerate the outline, so a brush change is a re-render and not a re-draw.
#[derive(Copy, Clone, Debug)]
struct InkPoint {
    at: Point,
    /// `PointerAxes::pressure` where the device reports one, else
    /// `PointerInfo::effective_pressure`'s W3C fallback.
    pressure: f32,
}

/// perfect-freehand's two knobs, minus the ones this demo does not need.
struct Brush {
    size: f32,
    /// `r(p) = size/2 * (1 - thinning * (1 - p))`.
    thinning: f32,
    /// Exponential smoothing applied to the **input**, before any geometry:
    /// `s[i] = s[i-1] + (raw[i] - s[i-1]) * (1 - streamline)`.
    streamline: f32,
}

const BRUSH: Brush = Brush {
    size: 9.0,
    thinning: 0.7,
    streamline: 0.45,
};

impl Brush {
    fn radius(&self, pressure: f32) -> f32 {
        self.size * 0.5 * (1.0 - self.thinning * (1.0 - pressure.clamp(0.0, 1.0)))
    }
}

/// The fill rule wet and dry share, named once so they cannot drift apart.
///
/// [`outline`] returns **one** closed polygon, and a variable-width ribbon that
/// crosses itself is one region — which is the whole reason perfect-freehand
/// emits a single closed outline rather than a stroked centreline. `Winding`
/// says so: the overlap winds twice, and twice is not zero, so it fills.
/// `EvenOdd` punches a hole through every loop and every crossing, and because
/// a [`PathItem`]'s fill rule drives its hit shape as well as its paint
/// (`items/path.rs`'s `shape.filled(self.fill_rule)`), the hole is not only
/// visible — a point-erase aimed at the crossing would miss it too.
const INK_FILL: FillRule = FillRule::Winding;

/// The closed outline polygon for a run of points.
///
/// Walk the spine offsetting each point perpendicular to the local tangent by
/// its own radius, up one side and back down the other.
///
/// **Half of the shape, not all of it.** This function and [`INK_FILL`] are
/// what wet and dry must share; either one differing changes the picture the
/// instant the stroke dries. A closed polygon only names a *boundary* — which
/// side of it is ink is the fill rule's answer, and the two rules disagree
/// exactly where the ribbon crosses itself: every loop, every cursive `e`,
/// every crossed-out word. Sharing the geometry and splitting the rule is the
/// classic ink bug wearing the shape of a fix.
fn outline(points: &[InkPoint], brush: &Brush) -> Path {
    let mut path = Path::new();
    if points.is_empty() {
        return path;
    }
    if points.len() == 1 {
        let r = brush.radius(points[0].pressure).max(0.4);
        let c = points[0].at;
        path.move_to(Point::new(c.x + r, c.y));
        path.arc_to(Rect::new(c.x - r, c.y - r, r * 2.0, r * 2.0), 0.0, 360.0);
        path.close();
        return path;
    }

    let normal_at = |i: usize| -> (f32, f32) {
        let prev = points[i.saturating_sub(1)].at;
        let next = points[(i + 1).min(points.len() - 1)].at;
        let (dx, dy) = (next.x - prev.x, next.y - prev.y);
        let len = dx.hypot(dy);
        if len < 1e-4 {
            (0.0, 1.0)
        } else {
            (-dy / len, dx / len)
        }
    };

    // Up one side…
    for (i, point) in points.iter().enumerate() {
        let (nx, ny) = normal_at(i);
        let r = brush.radius(point.pressure).max(0.4);
        let side = Point::new(point.at.x + nx * r, point.at.y + ny * r);
        if i == 0 {
            path.move_to(side);
        } else {
            path.line_to(side);
        }
    }
    // …and back down the other, which closes the outline.
    for (i, point) in points.iter().enumerate().rev() {
        let (nx, ny) = normal_at(i);
        let r = brush.radius(point.pressure).max(0.4);
        path.line_to(Point::new(point.at.x - nx * r, point.at.y - ny * r));
    }
    path.close();
    path
}

// ---------------------------------------------------------------------------
// The stroke in flight
// ---------------------------------------------------------------------------

/// How many points a committed chunk holds.
///
/// The whole cost story in one constant. Re-filling the *whole* growing outline
/// every frame is quadratic however cheap the cache key is, because the
/// rasterizer sees a longer path each time. Freezing the stroke into immutable
/// chunks as it goes turns all but the last `CHUNK` points into cache hits, and
/// a hit is O(1). Overlapping chunk seams are invisible for opaque ink; a
/// translucent highlighter has to pay for one outline instead (see
/// `docs/ink.md`).
const CHUNK: usize = 32;

#[derive(Default)]
struct WetStroke {
    /// Every sample, for the dry path and for the app's own persistence.
    all: Vec<InkPoint>,
    /// Frozen prefixes. Each is a `Path` the mask cache has already seen.
    chunks: Vec<Path>,
    /// The live tail — at most `CHUNK + 1` points, re-rasterized each frame.
    tail: Vec<InkPoint>,
    drawing: bool,
}

impl WetStroke {
    fn begin(&mut self, p: InkPoint) {
        self.all.clear();
        self.chunks.clear();
        self.tail.clear();
        self.drawing = true;
        self.extend(p);
    }

    fn extend(&mut self, raw: InkPoint) {
        // Streamline the *input*, before geometry, exactly as perfect-freehand
        // does — so wet and dry smooth identically.
        let p = match self.all.last() {
            Some(last) => InkPoint {
                at: Point::new(
                    last.at.x + (raw.at.x - last.at.x) * (1.0 - BRUSH.streamline),
                    last.at.y + (raw.at.y - last.at.y) * (1.0 - BRUSH.streamline),
                ),
                pressure: last.pressure + (raw.pressure - last.pressure) * 0.5,
            },
            None => raw,
        };
        self.all.push(p);
        self.tail.push(p);
        if self.tail.len() > CHUNK {
            self.chunks.push(outline(&self.tail, &BRUSH));
            // The new tail starts on the old one's last point, so the chunks
            // overlap by a segment and the seam has no gap.
            let last = *self.tail.last().expect("just pushed");
            self.tail.clear();
            self.tail.push(last);
        }
    }

    fn paint(&self, canvas: &mut Canvas, ctx: &SceneItemPaintContext<'_>) {
        if !self.drawing {
            return;
        }
        let ink = ctx.theme.colors.text_primary;
        for chunk in &self.chunks {
            canvas.fill_path_with_rule(chunk, ink, INK_FILL);
        }
        if self.tail.len() > 1 {
            canvas.fill_path_with_rule(&outline(&self.tail, &BRUSH), ink, INK_FILL);
        }
    }

    /// Finish, returning the points the dry item is built from.
    fn finish(&mut self) -> Vec<InkPoint> {
        self.drawing = false;
        self.chunks.clear();
        self.tail.clear();
        std::mem::take(&mut self.all)
    }
}

// ---------------------------------------------------------------------------
// The page
// ---------------------------------------------------------------------------

/// The `z` the ink layer lives at: above the note at `z = 0`, below the one at
/// `z = 20`.
///
/// The band is what makes that expressible at all — `Under` can only put ink
/// below *every* note, `Over` only above *every* note.
///
/// And it is only expressible because the notes below are **heavyweight**:
/// [`SceneLayer::Interleaved`] slots a lightweight item into the arena's child
/// walk, which is the heavyweight tier. It orders ink against cards. It does
/// not order it against the `Under` band — the whole of that band paints in
/// `SceneView::paint`, before the first child — so in a scene made only of
/// lightweight items an interleaved item is above everything, whatever its `z`.
/// That is the case this example would have demonstrated by accident had the
/// notes stayed lightweight, and the opposite of what it claims.
const INK_Z: f32 = 10.0;

/// One note: a **heavyweight** card in the scene — a real widget in the widget
/// arena, with a title and a body, the thing `Interleaved` orders ink against.
///
/// `hit_transparent` is the pen tool owning the page, and it is one line rather
/// than an oversight: with the pen down, a note is content to draw on, not a
/// control to click. It moves the **hit target** over a note from the card to
/// the view, which
/// `the_pen_owns_the_page_and_the_notes_do_not_take_the_hit` pins.
///
/// It does *not* decide whether the stroke gets drawn, and the difference is
/// worth stating because it is easy to claim the larger thing. These cards
/// carry no pointer handler, so a press on one bubbles to the view and the ink
/// tool sees it under either setting — delete this line and every test here
/// still passes but that one. What the target decides is everything resolved
/// against it rather than dispatched through it: the cursor, hover and tooltip,
/// and any handler a card grows later. A *selection* tool wants the opposite
/// answer, which is why this is a knob on the note and not a property of the
/// view.
fn note(model: &SceneModel, rect: Rect, title: &str, body: &str, z: f32, fill: Color) -> ItemId {
    let card = ZStack::new()
        .alignment(Alignment::TOP_LEADING)
        .child(
            RectWidget::new()
                .background(fill)
                .corner_radius(CornerRadius::uniform(8.0)),
        )
        .child(
            Padding::uniform(12.0).child(
                VStack::new()
                    .spacing(6.0)
                    .alignment(HAlignment::Leading)
                    .child(TextWidget::new(lit!(title.to_string())).style(TextStyleRole::BodyBold))
                    .child(TextWidget::new(lit!(body.to_string()))),
            ),
        )
        .hit_transparent(true);
    let id = model.add_widget(card, rect);
    model.set_z(id, z);
    id
}

fn build_view() -> (impl Widget + 'static, SceneModel, Rc<RefCell<Vec<ItemId>>>) {
    let model = SceneModel::new();

    note(
        &model,
        Rect::new(40.0, 40.0, 260.0, 150.0),
        "Under the ink (z = 0)",
        "A heavyweight card. Draw across it: the stroke covers it.",
        0.0,
        Color::new(0.94, 0.90, 0.72, 1.0),
    );
    note(
        &model,
        Rect::new(360.0, 200.0, 260.0, 150.0),
        "Over the ink (z = 20)",
        "Also a card, one step higher. Draw across it: it covers the stroke.",
        20.0,
        Color::new(0.80, 0.90, 0.95, 1.0),
    );

    let wet: Rc<RefCell<WetStroke>> = Rc::default();
    let strokes: Rc<RefCell<Vec<ItemId>>> = Rc::default();
    let view = attach_ink(
        SceneView::with_model(model.clone())
            .min_zoom(0.25)
            .max_zoom(4.0),
        model.clone(),
        wet,
        strokes.clone(),
    );
    (view, model, strokes)
}

/// Wire the ink tool onto a view: the wet surface, and the pointer handler that
/// feeds it.
///
/// Separate from [`build_view`] so the tests below can drive exactly this on a
/// view whose camera they choose — the projection is the half of an ink tool no
/// framework test can check for you.
fn attach_ink(
    view: SceneView,
    model: SceneModel,
    wet: Rc<RefCell<WetStroke>>,
    strokes: Rc<RefCell<Vec<ItemId>>>,
) -> impl Widget + 'static {
    let layer = {
        let wet = wet.clone();
        WetLayer::new(move |canvas, ctx| wet.borrow().paint(canvas, ctx))
    };

    let pointer_model = model;
    let pointer_wet = wet;
    let pointer_layer = layer.clone();
    let pointer_strokes = strokes;

    let base = view.wet_layer(layer);
    // The page pans and zooms under the pen, so the projection is read per
    // event rather than captured as a value: a stroke has to land where the tip
    // was, not where it would have been a frame ago. `position` reaches a
    // handler in **window** coordinates, and `coalesced` says in its own doc
    // that it does too — so both go through the same inverse.
    let view_xform = base.view_transform_signal();
    let to_scene = move |p: Point| {
        view_xform
            .get()
            .inverse()
            .map(|inv| inv.apply_point(p))
            .unwrap_or(Point::ZERO)
    };

    base.on_pointer_event(move |event, ctx| {
        match event {
            WidgetEvent::PointerDown { position, .. } => {
                pointer_wet.borrow_mut().begin(InkPoint {
                    at: to_scene(*position),
                    pressure: ctx.pointer().effective_pressure(),
                });
                pointer_layer.request_repaint(ctx);
                EventResponse::Handled
            }
            WidgetEvent::PointerMove { position, .. } => {
                if !pointer_wet.borrow().drawing {
                    return EventResponse::Ignored;
                }
                {
                    let mut wet = pointer_wet.borrow_mut();
                    // 1. every position the platform batched…
                    for c in ctx.coalesced() {
                        wet.extend(InkPoint {
                            at: to_scene(c.window_position),
                            pressure: c.axes.pressure.unwrap_or(0.5),
                        });
                    }
                    // …2. then the packet's own, which is the newest.
                    wet.extend(InkPoint {
                        at: to_scene(*position),
                        pressure: ctx.pointer().effective_pressure(),
                    });
                }
                // 3. one node repaints; the item bands do not.
                pointer_layer.request_repaint(ctx);
                EventResponse::Handled
            }
            WidgetEvent::PointerUp { .. } => {
                let points = pointer_wet.borrow_mut().finish();
                if points.len() > 1 {
                    // 4. dry: one lightweight item, in the band between the two
                    // notes, filled from the same geometry **and the same rule**
                    // the wet stroke used — see `INK_FILL`. A mask-based
                    // point-erase does not want a different rule: an erased
                    // region is a second path subtracted at composite time, not
                    // a reversed winding inside this one.
                    let id = pointer_model.add_item(
                        PathItem::new(outline(&points, &BRUSH))
                            .fill(TextRole::Primary)
                            .fill_rule(INK_FILL)
                            // A page of strokes must not become a page of
                            // unnamed graphics nodes: hide the stroke and let
                            // the layer carry one named node instead.
                            .access_hidden(true),
                        Point::ZERO,
                    );
                    pointer_model.set_layer(id, SceneLayer::Interleaved);
                    pointer_model.set_z(id, INK_Z);
                    pointer_strokes.borrow_mut().push(id);
                }
                pointer_layer.request_repaint(ctx);
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    })
}

fn build_root() -> impl Widget + 'static {
    let (view, model, strokes) = build_view();
    VStack::new()
        .spacing(8.0)
        .child(
            TextWidget::new(lit!("teksilo-scene ink — a stroke between two notes"))
                .style(TextStyleRole::BodyBold),
        )
        .child(TextWidget::new(lit!(
            "Draw across both notes — they are heavyweight cards, and the ink is \
             drawn over the sand-coloured one and under the blue one. Backspace \
             removes the last stroke."
        )))
        .child(
            Expand::new().child(
                ZStack::new()
                    .alignment(Alignment::TOP_LEADING)
                    .child(Expand::new().child(view)),
            ),
        )
        .focusable(true)
        .on_key(move |event, _ctx| {
            if let WidgetEvent::KeyDown {
                key: Key::Backspace,
                ..
            } = event
            {
                if let Some(id) = strokes.borrow_mut().pop() {
                    model.remove(id);
                }
                return EventResponse::Handled;
            }
            EventResponse::Ignored
        })
}

fn main() {
    TeksiloAppBuilder::new()
        .install_inspector_in_debug()
        // A digitizer outruns the window's message rate. `Coalesce` spends one
        // tree dispatch per drain and hands the intermediate positions to the
        // handler above through `ctx.coalesced()` — which is why that handler
        // reads them before it reads its own position.
        .pen_batching(teksilo_platform::PenBatching::Coalesce)
        .theme(intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Teksilo — scene ink")
                .size(900, 640)
                .root(|tree, _state| tree.add(build_root())),
        )
        .run();
}

/// Keeps the `Size` import honest — the outline math works in points, and a
/// size only appears if a brush ever grows a stamp.
#[allow(dead_code)]
fn _unused(s: Size) -> f32 {
    s.width
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo::canvas::{DrawCommand, RenderFrame, SizeProposal};
    use teksilo::core::{EventTime, PointerPhase, PointerSample, WidgetTree};

    const SAND: Color = Color::new(0.94, 0.90, 0.72, 1.0);
    const SKY: Color = Color::new(0.80, 0.90, 0.95, 1.0);

    /// Every solid fill in the frame, as `(position in the draw order, colour)`
    /// — across all three tiers, because a note is a rounded rect (an SDF
    /// `Shape` or a plain `Decoration`, depending on the style) and the ink is a
    /// `Path`. Comparing only one tier would compare a note against nothing.
    fn fills(frame: &RenderFrame) -> Vec<(usize, [f32; 4])> {
        frame
            .draw_order
            .iter()
            .enumerate()
            .filter_map(|(i, cmd)| match cmd {
                DrawCommand::Decoration(k) => Some((i, frame.decorations[*k].color)),
                DrawCommand::Shape(k) => Some((i, frame.shapes[*k].color)),
                DrawCommand::Path(k) => Some((i, frame.paths[*k].color)),
                _ => None,
            })
            .collect()
    }

    fn first_at(frame: &RenderFrame, colour: Color) -> Option<usize> {
        fills(frame)
            .into_iter()
            .find(|(_, c)| *c == colour.to_array())
            .map(|(i, _)| i)
    }

    /// The position of the one rasterized path in the frame — the dried stroke.
    fn path_at(frame: &RenderFrame) -> Option<usize> {
        frame
            .draw_order
            .iter()
            .position(|cmd| matches!(cmd, DrawCommand::Path(_)))
    }

    /// Draw a straight stroke from `from` to `to` on `tree`, in window
    /// coordinates.
    fn stroke(tree: &mut WidgetTree, from: Point, to: Point) {
        const STEPS: u64 = 24;
        tree.dispatch_pointer(
            PointerSample::mouse(PointerPhase::Down, from, EventTime::from_millis(0))
                .with_button(teksilo::core::PointerButton::Primary),
        );
        for i in 1..=STEPS {
            let t = i as f32 / STEPS as f32;
            tree.dispatch_pointer(PointerSample::mouse(
                PointerPhase::Move,
                Point::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t),
                EventTime::from_millis(i * 4),
            ));
        }
        tree.dispatch_pointer(PointerSample::mouse(
            PointerPhase::Up,
            to,
            EventTime::from_millis(STEPS * 4 + 4),
        ));
    }

    /// **The headline, asserted against the scene this example actually
    /// builds** — not against a bare model with the band field set.
    ///
    /// `SceneLayer::Interleaved` orders a lightweight item against the *cards*,
    /// which are the arena's children. So the claim "over the sand note, under
    /// the blue one" is a claim about a scene whose notes are heavyweight, and
    /// the only way to check it is to render the real page and read the draw
    /// order back.
    ///
    /// Make the notes lightweight again — `add_item` instead of `add_widget` in
    /// [`note`] — and the second assertion reddens: the whole `Under` band
    /// paints inside `SceneView::paint`, before the first child, so an
    /// interleaved stroke would be above *both* notes however the `z`s are set.
    #[test]
    fn the_page_paints_the_ink_between_its_two_notes() {
        let (view, _model, strokes) = build_view();
        let mut tree = WidgetTree::new();
        tree.add(view);
        tree.layout(SizeProposal::exact(700.0, 400.0));

        // Diagonally across both notes: (40..300, 40..190) and
        // (360..620, 200..350) in scene coordinates, which the unpanned,
        // unzoomed view leaves equal to window coordinates.
        stroke(&mut tree, Point::new(60.0, 60.0), Point::new(600.0, 320.0));
        assert_eq!(strokes.borrow().len(), 1, "one stroke dried");

        tree.layout(SizeProposal::exact(700.0, 400.0));
        let frame = tree.render();

        let sand_at = first_at(&frame, SAND).expect("the z=0 note painted");
        let sky_at = first_at(&frame, SKY).expect("the z=20 note painted");
        let ink_at = path_at(&frame).expect("the dried stroke painted");

        assert!(
            sand_at < ink_at,
            "the stroke (z = {INK_Z}) must paint over the z=0 note: sand at \
             {sand_at}, ink at {ink_at}"
        );
        assert!(
            ink_at < sky_at,
            "…and under the z=20 note: ink at {ink_at}, sky at {sky_at}. An \
             all-lightweight page fails here — Interleaved orders against the \
             cards, and there would be none"
        );
    }

    /// `hit_transparent` on the notes, pinned for what it actually does: the
    /// **hit target** over a note is the view, not the card.
    ///
    /// Delete the `.hit_transparent(true)` in [`note`] and this reddens with
    /// the card's own id. Nothing else in this example reddens, and saying so
    /// is the point — the press reaches the ink either way, because a card with
    /// no pointer handler of its own lets it bubble to the view. What the line
    /// buys is everything the *target* decides rather than the dispatch:
    /// the cursor, hover and tooltip that resolve against it, and any handler a
    /// card later grows.
    #[test]
    fn the_pen_owns_the_page_and_the_notes_do_not_take_the_hit() {
        let (view, _model, _strokes) = build_view();
        let mut tree = WidgetTree::new();
        let root = tree.add(view);
        tree.layout(SizeProposal::exact(700.0, 400.0));

        // Inside the z=0 note, which covers scene (40, 40)–(300, 190).
        let over_a_note = Point::new(120.0, 100.0);
        assert_eq!(
            tree.hit_test(over_a_note),
            Some(root),
            "with the pen down a note is content to draw on, not a control to \
             click, so the page owns the hit"
        );
    }

    /// The notes are heavyweight — real widgets in the arena — which is the
    /// only reason the ordering above is expressible, and worth pinning
    /// separately from the paint order it produces.
    #[test]
    fn the_notes_are_heavyweight_cards() {
        let (view, _model, _strokes) = build_view();
        let mut tree = WidgetTree::new();
        let root = tree.add(view);
        tree.layout(SizeProposal::exact(700.0, 400.0));
        // Two cards plus the wet layer's own paint node — and nothing else:
        // a lightweight note would be painted by the view itself and be no
        // child of it at all, which is why an all-lightweight page has nothing
        // for `Interleaved` to interleave with.
        assert_eq!(
            tree.children(root).len(),
            3,
            "expected two heavyweight cards and the wet node as the view's \
             arena children, got {:?}",
            tree.children(root)
        );
    }

    /// The one thing about this example a framework test cannot reach: that the
    /// **projection** is right. A handler is given window coordinates and the
    /// stroke has to land in scene coordinates, so a missing (or doubled)
    /// inverse puts the ink somewhere the pen never went — and nothing in the
    /// framework can tell.
    ///
    /// Pan the page, draw a straight line, and check the dried stroke's *scene*
    /// bounds against where the pointer was. Drop the `to_scene` calls in the
    /// handler and this fails by exactly the pan.
    #[test]
    fn a_stroke_lands_where_the_pointer_went_on_a_panned_page() {
        const PAN: f32 = -120.0;
        let model = SceneModel::new();
        let wet: Rc<RefCell<WetStroke>> = Rc::default();
        let strokes: Rc<RefCell<Vec<ItemId>>> = Rc::default();
        let view = attach_ink(
            SceneView::with_model(model.clone()).initial_pan(PAN, PAN),
            model.clone(),
            wet.clone(),
            strokes.clone(),
        );

        let mut tree = WidgetTree::new();
        tree.add(view);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let at = |x: f32| Point::new(x, 200.0);
        tree.dispatch_pointer(
            PointerSample::mouse(PointerPhase::Down, at(100.0), EventTime::from_millis(0))
                .with_button(teksilo::core::PointerButton::Primary),
        );
        for i in 1..=20 {
            tree.dispatch_pointer(PointerSample::mouse(
                PointerPhase::Move,
                at(100.0 + i as f32 * 5.0),
                EventTime::from_millis(i * 4),
            ));
        }
        tree.dispatch_pointer(PointerSample::mouse(
            PointerPhase::Up,
            at(200.0),
            EventTime::from_millis(100),
        ));

        let ids = strokes.borrow();
        assert_eq!(ids.len(), 1, "one stroke was drawn");
        let rect = model.scene_rect(ids[0]).expect("the stroke has geometry");

        // Window (100..200, 200) with the page panned by `PAN` is scene
        // (100-PAN .. 200-PAN, 200-PAN). Streamlining lags the input, so the
        // far end is checked as a lower bound rather than an equality.
        let expect_x0 = 100.0 - PAN;
        let expect_y = 200.0 - PAN;
        assert!(
            (rect.x - expect_x0).abs() < 8.0,
            "the stroke starts where the press was in SCENE coords: expected \
             x≈{expect_x0}, got {rect:?}. A missing inverse would put it at \
             x≈100."
        );
        assert!(
            (rect.y + rect.height * 0.5 - expect_y).abs() < 10.0,
            "…and on the same row: expected y≈{expect_y}, got {rect:?}"
        );
        assert!(
            rect.width > 50.0,
            "a 100 dp horizontal stroke is at least 50 dp wide after \
             streamlining, got {rect:?}"
        );
        assert_eq!(
            model.layer(ids[0]),
            Some(SceneLayer::Interleaved),
            "and it dried into the band between the notes"
        );
    }

    /// The polygon [`outline`] built, read back out of a rendered frame.
    ///
    /// It emits only `MoveTo` / `LineTo` / `Close`, which is what makes the two
    /// fill rules computable here without a rasterizer.
    fn polygon(path: &Path) -> Vec<Point> {
        use teksilo::canvas::PathCommand;
        path.commands()
            .iter()
            .filter_map(|c| match c {
                PathCommand::MoveTo(p) | PathCommand::LineTo(p) => Some(*p),
                _ => None,
            })
            .collect()
    }

    /// `> 0` when `p` is left of the directed edge `a → b`.
    fn is_left(a: Point, b: Point, p: Point) -> f32 {
        (b.x - a.x) * (p.y - a.y) - (p.x - a.x) * (b.y - a.y)
    }

    /// The signed number of times the closed polygon wraps `p`. `Winding` fills
    /// where this is non-zero.
    fn winding_number(poly: &[Point], p: Point) -> i32 {
        let mut wn = 0;
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            if a.y <= p.y {
                if b.y > p.y && is_left(a, b, p) > 0.0 {
                    wn += 1;
                }
            } else if b.y <= p.y && is_left(a, b, p) < 0.0 {
                wn -= 1;
            }
        }
        wn
    }

    /// How many edges a ray from `p` crosses. `EvenOdd` fills where this is odd.
    fn crossings(poly: &[Point], p: Point) -> u32 {
        let mut cn = 0;
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            if (a.y <= p.y) != (b.y <= p.y) {
                let t = (p.y - a.y) / (b.y - a.y);
                if p.x < a.x + t * (b.x - a.x) {
                    cn += 1;
                }
            }
        }
        cn
    }

    /// Every fill rule the frame's rasterized paths were filled with, in draw
    /// order.
    fn path_rules(frame: &RenderFrame) -> Vec<FillRule> {
        frame
            .draw_order
            .iter()
            .filter_map(|cmd| match cmd {
                DrawCommand::Path(k) => Some(frame.paths[*k].fill_rule),
                _ => None,
            })
            .collect()
    }

    /// **The stroke must not change shape when it dries**, and the geometry is
    /// only half of its shape: a closed outline names a boundary, and which
    /// side of it is ink is the fill rule's answer.
    ///
    /// Drawn as a figure eight, because that is the one shape the question has
    /// an answer for. The example's three other strokes are straight lines,
    /// where `Winding` and `EvenOdd` agree everywhere — which is exactly how a
    /// wet `Winding` and a dry `EvenOdd` shipped side by side.
    ///
    /// Give the dry item back its own rule — `.fill_rule(FillRule::EvenOdd)` at
    /// the `PointerUp` arm — and the first assertion reddens. Set [`INK_FILL`]
    /// to `EvenOdd` instead, so both agree on the *wrong* rule, and the last one
    /// does: the crossing would be a hole in both.
    #[test]
    fn wet_and_dry_fill_a_self_crossing_loop_the_same_way() {
        let model = SceneModel::new();
        let wet: Rc<RefCell<WetStroke>> = Rc::default();
        let strokes: Rc<RefCell<Vec<ItemId>>> = Rc::default();
        let view = attach_ink(
            SceneView::with_model(model.clone()),
            model.clone(),
            wet,
            strokes.clone(),
        );

        let mut tree = WidgetTree::new();
        tree.add(view);
        tree.layout(SizeProposal::exact(400.0, 400.0));

        // x = sin 2t, y = sin t — a figure eight whose spine passes through its
        // own centre twice, so the ribbon lies over itself there.
        const STEPS: u64 = 96;
        let at = |i: u64| {
            let t = i as f32 / STEPS as f32 * std::f32::consts::TAU;
            Point::new(200.0 + 90.0 * (2.0 * t).sin(), 200.0 + 70.0 * t.sin())
        };

        tree.dispatch_pointer(
            PointerSample::mouse(PointerPhase::Down, at(0), EventTime::from_millis(0))
                .with_button(teksilo::core::PointerButton::Primary),
        );
        for i in 1..=STEPS {
            tree.dispatch_pointer(PointerSample::mouse(
                PointerPhase::Move,
                at(i),
                EventTime::from_millis(i * 4),
            ));
        }

        let wet_rules = path_rules(&tree.render());
        assert!(
            !wet_rules.is_empty(),
            "the wet stroke painted at least one path before it dried"
        );

        tree.dispatch_pointer(PointerSample::mouse(
            PointerPhase::Up,
            at(STEPS),
            EventTime::from_millis(STEPS * 4 + 4),
        ));
        assert_eq!(strokes.borrow().len(), 1, "one stroke dried");
        tree.layout(SizeProposal::exact(400.0, 400.0));
        let dry_frame = tree.render();
        let dry_rules = path_rules(&dry_frame);
        assert_eq!(
            dry_rules.len(),
            1,
            "the dried stroke is the frame's only path: {dry_rules:?}"
        );

        assert!(
            wet_rules.iter().all(|r| *r == dry_rules[0]),
            "wet filled with {wet_rules:?} and dry with {:?}. The rule is half \
             the shape: sharing only `outline` lets the picture change at the \
             instant of the PointerUp",
            dry_rules[0]
        );

        // …and this stroke is one the rules genuinely disagree about, so the
        // assertion above cannot pass by drawing something too simple to care.
        let DrawCommand::Path(k) = dry_frame
            .draw_order
            .iter()
            .find(|c| matches!(c, DrawCommand::Path(_)))
            .expect("the dried stroke painted")
        else {
            unreachable!("just matched")
        };
        let poly = polygon(&dry_frame.paths[*k].path);
        let disputed = (0..200)
            .flat_map(|ix| (0..200).map(move |iy| Point::new(110.0 + ix as f32, 130.0 + iy as f32)))
            .find(|p| (winding_number(&poly, *p) != 0) != (crossings(&poly, *p) % 2 == 1));
        let disputed = disputed.expect(
            "the figure eight was expected to overlap itself; if it no longer \
             does, this test proves nothing and the stroke needs rewriting \
             rather than this line deleting",
        );

        assert_ne!(
            winding_number(&poly, disputed),
            0,
            "at {disputed:?} the ribbon lies over itself (winding {}, {} \
             crossings). A self-crossing ribbon is one region, not a region \
             with a hole in it — which is why `INK_FILL` is `Winding` and why \
             perfect-freehand emits a single closed outline",
            winding_number(&poly, disputed),
            crossings(&poly, disputed),
        );
        assert_eq!(
            dry_rules[0],
            FillRule::Winding,
            "…and the stroke is filled by the rule that says so"
        );
    }

    /// Every position a batch carried reaches the stroke, not one per packet.
    #[test]
    fn a_batched_move_contributes_every_position() {
        use teksilo::core::{CoalescedSample, PointerInfo};

        let model = SceneModel::new();
        let wet: Rc<RefCell<WetStroke>> = Rc::default();
        let strokes: Rc<RefCell<Vec<ItemId>>> = Rc::default();
        let view = attach_ink(
            SceneView::with_model(model.clone()),
            model.clone(),
            wet.clone(),
            strokes.clone(),
        );

        let mut tree = WidgetTree::new();
        tree.add(view);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        tree.dispatch_pointer(
            PointerSample::mouse(PointerPhase::Down, Point::new(10.0, 10.0), EventTime::ZERO)
                .with_button(teksilo::core::PointerButton::Primary),
        );
        let mut sample = PointerSample::mouse(
            PointerPhase::Move,
            Point::new(40.0, 10.0),
            EventTime::from_millis(12),
        );
        sample.pointer = PointerInfo::mouse(EventTime::from_millis(12));
        sample.coalesced = vec![
            CoalescedSample::new(EventTime::from_millis(4), Point::new(20.0, 10.0)),
            CoalescedSample::new(EventTime::from_millis(8), Point::new(30.0, 10.0)),
        ];
        tree.dispatch_pointer(sample);

        assert_eq!(
            wet.borrow().all.len(),
            4,
            "the press plus three positions — two batched and the packet's own"
        );
    }
}
