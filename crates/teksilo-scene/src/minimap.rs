// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`SceneMinimap`] — a small thumbnail of a [`Scene`](crate::Scene)
//! showing all items as dots / rects scaled down, with an overlay
//! highlighting the currently visible viewport rectangle.
//!
//! ## Use
//!
//! ```
//! use teksilo_scene::{Scene, SceneView, SceneMinimap};
//! use teksilo_canvas::Rect;
//! # use teksilo_widgets::VStack;
//!
//! let mut scene = Scene::new();
//! /* …populate scene… */
//! // Build the SceneView FIRST so we can read its reactive
//! // viewport signal and its scene's snapshot of items.
//! let view = SceneView::new(scene);
//! let content = view
//!     .scene_content_bounds()
//!     .unwrap_or(Rect::new(0.0, 0.0, 1000.0, 1000.0));
//! let viewport_signal = view.viewport_in_scene_signal();
//! let item_thumbs = view.scene().item_thumbnails(); // Vec<(Rect, Color)>
//!
//! let _w = VStack::new()
//!     .child(view)
//!     .child(
//!         SceneMinimap::new(content, viewport_signal)
//!             .items(item_thumbs)
//!             .size(200.0, 150.0),
//!     );
//! ```
//!
//! For a live "items as they move" minimap, re-call
//! [`Scene::item_thumbnails`](crate::Scene::item_thumbnails) on
//! scene mutations and rebuild the widget tree (or wire a
//! `Signal<Vec<(Rect, Color)>>` if your app needs per-frame
//! reactivity).
//!
//! ## Design
//!
//! Deliberately decoupled from `SceneView`: it doesn't reach into
//! the scene model. Instead it consumes a content extent (the rect
//! the minimap is guaranteed to show), a static `Vec<(Rect, Color)>`
//! of item thumbnails (refreshed by the app whenever items move),
//! and a `Signal<Rect>` for the live viewport rectangle.
//!
//! Apps that want a live "items as they move" minimap rebuild their
//! widget tree on scene mutations or wire a `Signal<Vec<...>>`. The
//! viewport overlay is reactive on its own — the minimap re-paints
//! whenever the SceneView's pan / zoom changes, with no manual
//! plumbing.
//!
//! ## The projection, and why it expands
//!
//! `content_bounds` is a *floor*, not a frame. The rect actually
//! projected onto the drawing area is the **effective extent**:
//!
//! ```text
//! effective_extent = content_bounds ∪ every item rect ∪ viewport_in_scene
//! ```
//!
//! This matters because the inputs are unrelated. `content_bounds`
//! is typically the union of item rects
//! (`SceneView::scene_content_bounds`), while `viewport_in_scene` is
//! the viewport projected through the inverse view transform — and
//! pan is **unbounded by default** (`Scene::current_pan_bounds()` is
//! `None`). A user who zooms in and pans away, or who zooms out past
//! the content, puts the viewport wholly outside `content_bounds`. A
//! minimap that mapped `content_bounds` alone would then draw the
//! indicator outside its own frame.
//!
//! Three answers were possible; this is the one that keeps the
//! minimap *useful*:
//!
//! - **Clamp** the indicator to the frame — it would lie about both
//!   position and size, and stop moving while the user is still
//!   panning.
//! - **Crop** it — honest, but the indicator vanishes exactly when
//!   the user most needs to know where they are.
//! - **Expand the extent** — position and size stay truthful and the
//!   content visibly shrinks as you wander off, which *is* the
//!   "you are out here, the content is over there" signal. This is
//!   what tldraw does (`Box.Expand(contentBounds, viewportBounds)`).
//!
//! The projection is a **uniform** fit: one scale for both axes,
//! centred in the drawing area, so a square item reads as a square
//! and the picture shrinks evenly as the extent grows. Expect
//! letterbox margins when the widget's aspect ratio differs from the
//! extent's.
//!
//! ## The clip
//!
//! `paint` clips **everything it emits** — background, outline,
//! thumbnails, indicator and the widget's own border — to the widget's
//! bounds. Not one stroke is drawn outside it, so "nothing the minimap
//! paints leaves its frame" holds without qualification and for every
//! caller-supplied width.
//!
//! That is a structural backstop, not a duplicate of the extent
//! expansion: the expansion is a *policy* a future edit could regress,
//! while the clip is a guarantee.
//!
//! It is also the only mechanism available here.
//! `Widget::clips_children` does **not** clip a widget's own
//! `paint()`: the render walker emits a plain clipping node's
//! `SetClip` *after* that node's paint (see
//! `teksilo-core/src/widget_tree/rendering_impl.rs`), so for a
//! self-painting leaf with no children it is a complete no-op. The
//! supported primitive is `Canvas::set_clip` / `clear_clip` inside
//! `paint`, as `ListView`, `TableView`, `CodeEditor` and `Terminal`
//! already do.
//!
//! The clip is set **after** the canvas has been translated to
//! `bounds.origin`, and is given the widget-*local* area — so it lands
//! on the widget wherever the parent placed it. At the root of a tree
//! that translation is the identity and the ordering is unobservable,
//! which is why the tests exercise a nested, offset minimap too.
//!
//! ## The border is inside the frame
//!
//! `Canvas::stroke_rect` centres each edge line on the rect boundary,
//! so stroking the widget area directly would hang half of the width
//! outside the widget — 12 px on every side for a 24 px border, over
//! whatever sibling is underneath. The border is therefore stroked on
//! the area **inset by half its width**, which puts the band wholly
//! inside: the CSS `border-box` convention, and the one every desktop
//! toolkit uses. A wide border eats into the picture rather than into
//! the neighbours; the clip then bounds even a pathological width.
//!
//! Clamping the width instead was rejected: it would silently render
//! something other than what the caller asked for, and it would still
//! leave the last half-pixel straddling the edge.
//!
//! ## Accessibility
//!
//! The minimap's whole job is to say *where you are* and let you go
//! somewhere else. Both halves are on the AT node, and the second half
//! has three input routes that all end in the same callback.
//!
//! **The node.** `Role::Group`, named `"Scene minimap"` by default,
//! carrying the position as its **value**. A 2-D position has no ARIA
//! role — there is no `slider2d` — and `numeric_value` holds one number
//! where this control has four, so the reading is text, exactly as
//! `HsvCanvas` (`teksilo-widgets/src/color_picker/hsv_canvas.rs`, the
//! workspace's other 2-D picker) does it. It is deliberately not a property-less
//! `Role::GenericContainer`: the walker prunes those, and a pruned node
//! announces nothing.
//!
//! **The value** is recomputed at every AT walk from the same
//! `effective_extent` the picture is drawn from, so it is the picture in
//! words: *"Viewport at 42% across, 17% down; showing 25% of the width and
//! 33% of the height"*. The viewport signal is bound **twice** — at
//! `RepaintOnly` for the drawing and at `AccessibilityOnly` for this — so a
//! pan updates the announcement as well as the overlay, with no rebuild.
//! Pass [`SceneMinimap::access_readout`] to phrase (and translate) it
//! yourself; the numbers arrive as a [`MinimapReadout`].
//!
//! **The routes.** With an [`on_click`](SceneMinimap::on_click) callback
//! installed the node is focusable and offers, all landing in that one
//! callback:
//!
//! | Route | Pointer | Keyboard | Assistive technology |
//! | --- | --- | --- | --- |
//! | move the view | tap a point | arrows (`Shift` = a whole viewport) | `ScrollLeft` / `Right` / `Up` / `Down` |
//! | centre on the content | — | `Home`, `Enter`, `Space` | `Click` |
//!
//! `Action::Click` is the position-free half of the tap: a click needs a
//! point and an AT client has none, so the primary action is the one
//! well-defined destination the widget can name on its own — the centre of
//! the content. `Home` / `Enter` / `Space` are bound to it so the keyboard
//! and the AT client do the same thing.
//!
//! Arrow steps are a fraction of the **viewport**, not of the scene: a
//! tenth of it for a nudge and a whole one for `Shift` — the line-vs-page
//! pair a scroll view uses, scaled to what the user is actually looking at
//! rather than to a count of scene units that would be a screenful at one
//! zoom and a hair at another. They are **not** mirrored under RTL, for
//! the reason `HsvCanvas` gives: the picture is not mirrored either, so an
//! arrow that flipped would disagree with what the user sees.
//!
//! **Who speaks.** The keyboard and assistive-technology routes announce the
//! move through [`EventContext::announce`]. The pointer route does not.
//!
//! An arrow press on an unannotated graphic tells a screen-reader user
//! nothing at all, and an AT client that invokes `Click` was never told where
//! "the content" is — for those two routes the utterance is the entire
//! feedback. A tap already has some: the user picked the destination by
//! aiming at the picture. And a minimap invites the gesture *repeatedly* —
//! click, look, click again — so speaking every one of them turns a polite
//! live region into a metronome talking over whatever the user was reading.
//!
//! That is the rule the rest of the workspace already follows, from both
//! directions. `HsvCanvas` — the other 2-D manipulator, and the widget this
//! one is modelled on — announces from its arrows and its custom actions and
//! not from its drag or its tap. The five data views' row reorder announces
//! from `teksilo_widgets::common::ordered_move`, which is the **non-drag**
//! alternative; the drop itself is silent. The one pointer route in the
//! workspace that does speak is the charts' readout
//! (`teksilo_charts::hit::drive_readout`), and only for a **coarse** pointer
//! that pressed and released without travelling — because that tap is an
//! *inspection* standing in for a hover a finger cannot perform, so the
//! utterance is its whole product, and a scrub is deliberately coalesced into
//! nothing. A minimap tap is a *command* whose destination the user chose, so
//! that exception does not reach it.
//!
//! Silence here is not lost information. The node's **value** is the same
//! sentence, and the `AccessibilityOnly` binding refreshes it the moment the
//! app applies the pan — so a client that re-reads the control after a click
//! gets the new position, it simply is not interrupted with it.
//!
//! What *is* announced describes the viewport the minimap **asked for**, not
//! one read back afterwards: the app owns the move (a `with_widget_mut` pan
//! lands after the handler returns, an animated one later still), so
//! re-reading the signal here would announce the position we just left. That
//! is why [`on_click`](SceneMinimap::on_click)'s contract is specifically
//! *"centre the view on this scene point"* rather than "here is a point, do
//! as you like".
//!
//! **Without `on_click`** the minimap is a read-out, not a control: it still
//! emits the named, valued node — the useful half — but takes no focus and
//! advertises no action.
//!
//! That gate is worth arguing rather than assuming, because a minimap
//! *displays* something and a display is worth reaching. The answer is that
//! reaching it and focusing it are separate questions. The node is in the
//! accessibility tree carrying a name and a value, so a screen reader's
//! object / browse navigation and its rotor all arrive at it and read the
//! position out; what the gate withholds is the **Tab** order, and Tab is for
//! things you can operate. A read-only minimap answers Tab with nothing: no
//! arrow does anything, `Enter` does nothing, no action is advertised, and
//! the focus ring has parked on a picture. That is the dead stop the ARIA
//! practices warn about, and it costs every keyboard user a press on the way
//! past.
//!
//! An app that wants it anyway is not blocked — `.focusable(true)` from the
//! framework's ordinary
//! [`WidgetBuilder`](teksilo_core::widget_builder::WidgetBuilder) chain puts
//! any widget into the Tab order, this one included. The default is the
//! answer that is right without knowing the app.
//!
//! **Overrides** come from the framework chain, not from a second set of
//! builders here: `.access_label(tr!(…))` renames the node (re-resolved on
//! every AT walk, so it follows a locale change),
//! `.access_description(…)` adds long-form context, and the rest of
//! `docs/accessibility-overrides.md` applies unchanged.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal, StrokeStyle};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accesskit::{Action, Role};
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, Modifiers, WidgetEvent};
use teksilo_core::signal::Signal;
use teksilo_core::widget::{
    CursorIcon, EventContext, LayoutContext, LayoutResponse, PaintContext, Widget,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::{LocalizedString, lit};
use teksilo_tokens::Color;

/// Stroke width of the viewport indicator, in minimap-local pixels.
/// `stroke_rect` centres each edge line on the boundary, so the
/// projection area is inset by half of this — otherwise an indicator
/// sitting exactly on the extent edge (the zoomed-all-the-way-out
/// case) would have its outer half shaved by the clip.
const VIEWPORT_STROKE_WIDTH: f32 = 2.0;

/// Default minimap size in widget-local pixels, as `(width, height)`.
const DEFAULT_SIZE: (f32, f32) = (200.0, 150.0);

/// The AT name a minimap carries unless the app replaces it with
/// `.access_label(...)`. A `lit!` default rather than a `tr!` one because
/// this crate ships no Fluent bundle; the override is the translation seam.
fn default_access_label() -> LocalizedString {
    lit!("Scene minimap")
}

/// One arrow press, as a fraction of the viewport's own extent along the
/// axis it moves — a scroll view's *line* step, scaled to what the user is
/// looking at instead of to a fixed count of scene units.
const NUDGE_FRACTION: f32 = 0.1;

/// `Shift` + arrow: a whole viewport — the *page* step.
const PAGE_FRACTION: f32 = 1.0;

/// True when every component of `r` is finite. A non-finite rect
/// would poison the projection (and, through `SetClip`, the
/// renderer's scissor stack), so such rects are dropped rather than
/// projected.
fn rect_is_finite(r: Rect) -> bool {
    r.x.is_finite() && r.y.is_finite() && r.width.is_finite() && r.height.is_finite()
}

/// True when `width` is a stroke worth emitting.
///
/// Both stroke widths here are caller-supplied (`border`,
/// `content_outline`) and reach `Canvas::draw_line`, which bakes the
/// width straight into a decoration rect: `NaN` would put a non-finite
/// rect in the frame, and a negative width an inside-out one. Every
/// other input to this widget is screened by [`rect_is_finite`]; these
/// two are the numbers that screening missed. A zero width draws
/// nothing anyway, so it is dropped with them rather than emitting four
/// invisible lines.
fn is_drawable_width(width: f32) -> bool {
    width.is_finite() && width > 0.0
}

/// AABB union of two rects, skipping a non-finite operand.
fn union_extent(a: Rect, b: Rect) -> Rect {
    if !rect_is_finite(b) {
        return a;
    }
    if !rect_is_finite(a) {
        return b;
    }
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let right = a.right().max(b.right());
    let bottom = a.bottom().max(b.bottom());
    Rect::new(x, y, right - x, bottom - y)
}

/// The similarity transform mapping scene coordinates onto the
/// minimap's drawing area: one uniform `scale` plus a translation.
///
/// Uniform rather than per-axis so the thumbnails and the viewport
/// indicator keep their real proportions, and so a growing extent
/// reads as an even zoom-out rather than an anisotropic warp.
#[derive(Clone, Copy, Debug, PartialEq)]
struct MinimapProjection {
    scale: f32,
    offset_x: f32,
    offset_y: f32,
}

impl MinimapProjection {
    /// Fit `extent` (scene coords) inside `area` (minimap-local
    /// coords), centred, preserving aspect ratio.
    ///
    /// Total: a degenerate axis (zero extent, or zero area)
    /// contributes no scale rather than a division by zero, a wholly
    /// degenerate extent falls back to 1:1 about the area centre, and
    /// a non-finite extent is treated as `Rect::ZERO`. The result's
    /// `scale` is always finite and strictly positive, so
    /// [`Self::invert`] is always well-defined.
    fn fit(extent: Rect, area: Rect) -> Self {
        let extent = if rect_is_finite(extent) {
            extent
        } else {
            Rect::ZERO
        };
        let sx = if extent.width > 0.0 && area.width > 0.0 {
            Some(area.width / extent.width)
        } else {
            None
        };
        let sy = if extent.height > 0.0 && area.height > 0.0 {
            Some(area.height / extent.height)
        } else {
            None
        };
        let scale = match (sx, sy) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => 1.0,
        };
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let centre = extent.center();
        let target = area.center();
        Self {
            scale,
            offset_x: target.x - centre.x * scale,
            offset_y: target.y - centre.y * scale,
        }
    }

    /// Scene point → minimap-local point.
    fn point(&self, p: Point) -> Point {
        Point::new(
            p.x * self.scale + self.offset_x,
            p.y * self.scale + self.offset_y,
        )
    }

    /// Minimap-local point → scene point. Exact inverse of
    /// [`Self::point`].
    fn invert(&self, p: Point) -> Point {
        Point::new(
            (p.x - self.offset_x) / self.scale,
            (p.y - self.offset_y) / self.scale,
        )
    }

    /// Scene rect → minimap-local rect, floored at one pixel per axis
    /// so a sub-pixel item still shows as a dot instead of vanishing.
    fn rect(&self, r: Rect) -> Rect {
        let tl = self.point(Point::new(r.x, r.y));
        Rect::new(
            tl.x,
            tl.y,
            (r.width * self.scale).max(1.0),
            (r.height * self.scale).max(1.0),
        )
    }
}

/// A small thumbnail rendering of a [`Scene`](crate::Scene)'s
/// content, with the live viewport rectangle highlighted.
///
/// Paint order: **clip on** → background fill → optional
/// content-bounds outline → item thumbnails (dots / rects) →
/// viewport overlay rect → inset border → **clip off**. The clip
/// covers every one of those, the border included.
///
/// The projected extent is `content_bounds` expanded to contain every
/// item thumbnail and the live viewport — see this module's
/// documentation ("The projection, and why it expands") for why, and
/// `docs/teksilo-scene.md` § Minimap for the same story in prose.
pub struct SceneMinimap {
    /// The scene-coord rect the minimap is guaranteed to show. Apps
    /// typically use `SceneView::scene_content_bounds()` or a
    /// hand-picked extent (e.g. `Rect::new(0,0, 10_000, 10_000)`
    /// for a known canvas).
    ///
    /// This is a **floor**: the minimap expands it to contain the
    /// item thumbnails and the live viewport, so nothing it draws can
    /// fall outside the picture. Pass a larger rect to keep the scale
    /// steady while the user pans inside it.
    content_bounds: Rect,
    /// `content_bounds` unioned with every item rect. Constant for
    /// the widget's lifetime (both inputs are set at construction),
    /// so it is folded once instead of per paint.
    static_extent: Rect,
    /// Live viewport rectangle in scene coords. Bound **twice** in
    /// `build`: at `RepaintOnly` so the overlay re-renders whenever the
    /// SceneView pans / zooms, and at `AccessibilityOnly` so the node's
    /// announced position follows the same move.
    viewport_in_scene: Signal<Rect>,
    /// Static snapshot of items + their thumbnail color. Apps
    /// refresh by rebuilding when items move.
    items: Vec<(Rect, Color)>,
    /// Minimap dimensions in widget-local pixels. Defaults to
    /// 200×150.
    size: Size,
    /// The projection the most recent `paint` used — **the** single
    /// source of truth shared by paint and tap.
    ///
    /// A tap inverts this exact transform rather than rebuilding one
    /// from the extent and the drawing area, so paint and click cannot
    /// disagree: there is nothing to disagree about. (Recomputing it
    /// meant two copies of the extent formula and two of the area, and
    /// a drift between them is invisible to any test that checks only
    /// one side.) It also makes the tap answer the right question — it
    /// inverts the frame the user is looking at.
    ///
    /// Seeded in `build`; `paint` takes `&self`, hence the `Cell`, and
    /// the tap closure is `'static`, hence the `Rc`.
    painted_projection: Rc<Cell<MinimapProjection>>,
    /// Background fill color. Defaults to a translucent white.
    background: Color,
    /// Border around the minimap drawing area, drawn **inside** the
    /// widget (see the module docs). Default 1px black.
    border: Option<(Color, f32)>,
    /// Color of the viewport overlay rectangle. Default solid blue,
    /// drawn as a stroke.
    viewport_color: Color,
    /// Optional outline of the content extent (gives users a sense
    /// of "you're inside this much scene"). Default `None`.
    content_outline: Option<(Color, f32)>,
    /// Optional navigation handler: the app is asked to centre the view
    /// on this scene point. Reached by the pointer, the keyboard and
    /// assistive technology alike; its presence is also what makes the
    /// widget focusable and action-bearing.
    on_click: Option<Rc<dyn Fn(Point, &mut EventContext)>>,
    /// App-supplied phrasing for the accessible readout. `None` uses
    /// [`default_readout_text`].
    readout: Option<Rc<dyn Fn(MinimapReadout) -> LocalizedString>>,
}

impl std::fmt::Debug for SceneMinimap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneMinimap")
            .field("content_bounds", &self.content_bounds)
            .field("static_extent", &self.static_extent)
            .field("size", &self.size)
            .field("item_count", &self.items.len())
            .field("on_click", &self.on_click.is_some())
            .field("access_readout", &self.readout.is_some())
            .finish_non_exhaustive()
    }
}

/// Where the viewport sits inside the picture the minimap is drawing — the
/// numbers behind the AT node's announced value, as plain fractions.
///
/// Handed to an [`access_readout`](SceneMinimap::access_readout) closure so an
/// app can phrase and translate the announcement itself. Everything here is
/// derived from the same `effective_extent` the picture is projected through,
/// so a readout can never describe a frame other than the one on screen.
///
/// `#[non_exhaustive]`: the crate hands this *to* consumer code — it is the
/// argument of an `access_readout` closure — and a readout may learn to carry
/// another number. Build one with [`new`](Self::new).
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct MinimapReadout {
    /// The scene-space rect actually projected onto the drawing area:
    /// `content_bounds ∪ every item rect ∪ viewport`.
    pub extent: Rect,
    /// The viewport rect, in scene coordinates.
    pub viewport: Rect,
    /// The viewport centre's position inside [`extent`](Self::extent), per
    /// axis, in `0.0..=1.0` (leading/top → trailing/bottom). A degenerate
    /// axis reads `0.5` — dead centre of a scene with no extent along it.
    pub position: Point,
    /// How much of [`extent`](Self::extent) the viewport covers, per axis, in
    /// `0.0..=1.0`. A degenerate axis reads `1.0`: you are seeing all there
    /// is of it.
    pub coverage: Size,
}

impl MinimapReadout {
    /// A readout stated field by field — the constructor
    /// [`#[non_exhaustive]`](Self) takes the place of a struct literal for.
    /// The minimap derives its own; this is for a consumer testing its own
    /// phrasing closure.
    pub fn new(extent: Rect, viewport: Rect, position: Point, coverage: Size) -> Self {
        Self {
            extent,
            viewport,
            position,
            coverage,
        }
    }
}

/// Derive the readout for a viewport inside an extent.
///
/// Total by construction: a non-finite extent collapses to `Rect::ZERO`, a
/// non-finite viewport falls back to the extent (you are looking at all of
/// it), and a degenerate axis answers `0.5` / `1.0` rather than dividing by
/// zero. Every field it returns is finite and in range.
fn readout_of(extent: Rect, viewport: Rect) -> MinimapReadout {
    let extent = if rect_is_finite(extent) {
        extent
    } else {
        Rect::ZERO
    };
    let viewport = if rect_is_finite(viewport) {
        viewport
    } else {
        extent
    };
    // `(centre fraction, coverage fraction)` for one axis.
    let axis = |centre: f32, span: f32, origin: f32, total: f32| -> (f32, f32) {
        if total > 0.0 {
            (
                ((centre - origin) / total).clamp(0.0, 1.0),
                (span / total).clamp(0.0, 1.0),
            )
        } else {
            (0.5, 1.0)
        }
    };
    let centre = viewport.center();
    let (px, cw) = axis(centre.x, viewport.width, extent.x, extent.width);
    let (py, ch) = axis(centre.y, viewport.height, extent.y, extent.height);
    MinimapReadout {
        extent,
        viewport,
        position: Point::new(px, py),
        coverage: Size::new(cw, ch),
    }
}

/// The default English phrasing of a [`MinimapReadout`].
///
/// Position first because that is what an arrow press changes; coverage
/// second because it is what a zoom changes. Percentages rather than scene
/// units: the units are the app's and may be metres, characters or nothing
/// at all, while "42% across" is true in every scene.
fn default_readout_text(r: MinimapReadout) -> String {
    let pct = |f: f32| (f * 100.0).round() as i32;
    format!(
        "Viewport at {}% across, {}% down; showing {}% of the width and {}% of the height",
        pct(r.position.x),
        pct(r.position.y),
        pct(r.coverage.width),
        pct(r.coverage.height)
    )
}

/// One navigation step, in the direction the arrows name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MinimapStep {
    Leading,
    Trailing,
    Up,
    Down,
}

impl MinimapStep {
    /// The arrow that means this step.
    ///
    /// Deliberately **not** mirrored under RTL: the minimap paints its scene
    /// left-to-right whatever the reading direction, so an arrow that flipped
    /// would disagree with the picture the user is looking at. Same reasoning
    /// as `HsvCanvas`, and the opposite of `Slider`, which mirrors because
    /// its paint does.
    fn from_key(key: Key) -> Option<Self> {
        match key {
            Key::ArrowLeft => Some(Self::Leading),
            Key::ArrowRight => Some(Self::Trailing),
            Key::ArrowUp => Some(Self::Up),
            Key::ArrowDown => Some(Self::Down),
            _ => None,
        }
    }

    /// The AccessKit scroll action that means this step. Real actions rather
    /// than four custom ones: every platform adapter already maps
    /// `ScrollLeft` and friends, and "scroll the view left" is exactly what
    /// this does — there is nothing here AccessKit has no word for.
    fn from_action(action: Action) -> Option<Self> {
        match action {
            Action::ScrollLeft => Some(Self::Leading),
            Action::ScrollRight => Some(Self::Trailing),
            Action::ScrollUp => Some(Self::Up),
            Action::ScrollDown => Some(Self::Down),
            _ => None,
        }
    }
}

/// The one place a navigation request becomes a scene point and reaches the
/// app — shared by the pointer, the keyboard and assistive technology, so
/// the three routes cannot drift into meaning different things.
///
/// Holds copies rather than a borrow of the widget: the handlers it feeds are
/// `'static`, and everything it needs is either `Copy` (`static_extent`) or a
/// shared handle (`Signal`, `Rc`).
struct MinimapNav {
    /// `content_bounds ∪ every item rect`, as the widget folded it once.
    static_extent: Rect,
    /// The live viewport, read afresh on every request.
    viewport: Signal<Rect>,
    /// App-supplied phrasing for the announcement, if any.
    readout: Option<Rc<dyn Fn(MinimapReadout) -> LocalizedString>>,
    /// The app's navigation callback — the destination of every route.
    on_click: Rc<dyn Fn(Point, &mut EventContext)>,
}

impl MinimapNav {
    /// The live viewport, screened: a non-finite signal value would poison
    /// every number downstream of it.
    fn live_viewport(&self) -> Rect {
        let v = self.viewport.get();
        if rect_is_finite(v) { v } else { Rect::ZERO }
    }

    /// The announcement for a viewport inside an extent.
    fn text_for(&self, extent: Rect, viewport: Rect) -> String {
        let r = readout_of(extent, viewport);
        match &self.readout {
            Some(f) => f(r).resolve_now(),
            None => default_readout_text(r),
        }
    }

    /// Ask the app to centre the view on `target`. **Silent** — this is the
    /// pointer route's door.
    ///
    /// Returns whether the request reached the app: a non-finite target is a
    /// poisoned projection rather than a destination, and is dropped instead
    /// of handed on.
    fn go_to(&self, target: Point, ctx: &mut EventContext) -> bool {
        if !target.x.is_finite() || !target.y.is_finite() {
            return false;
        }
        (self.on_click)(target, ctx);
        true
    }

    /// [`Self::go_to`], plus the utterance the keyboard and
    /// assistive-technology routes owe — see the module's "Accessibility" for
    /// why the pointer route does not come through here.
    ///
    /// The announcement is computed from the request, not read back from the
    /// signal afterwards. The app owns the move and may not have made it yet
    /// — `ctx.with_widget_mut` applies after the handler returns, and an
    /// animated pan later still — so reading the signal here would announce
    /// the position the user just left, on every single keypress.
    ///
    /// A request that was dropped says nothing: "moved" when nothing moved is
    /// worse than silence.
    fn go_to_announcing(&self, target: Point, ctx: &mut EventContext) {
        if !self.go_to(target, ctx) {
            return;
        }
        let mut moved = self.live_viewport();
        moved.x = target.x - moved.width * 0.5;
        moved.y = target.y - moved.height * 0.5;
        let extent = union_extent(self.static_extent, moved);
        ctx.announce(self.text_for(extent, moved));
    }

    /// Move the view one step. `coarse` is `Shift` — a whole viewport
    /// instead of a tenth of one.
    fn step(&self, step: MinimapStep, coarse: bool, ctx: &mut EventContext) {
        let viewport = self.live_viewport();
        let extent = union_extent(self.static_extent, viewport);
        let fraction = if coarse {
            PAGE_FRACTION
        } else {
            NUDGE_FRACTION
        };
        // A viewport with no extent of its own (before the first layout, or
        // a degenerate view) steps by the scene instead, so the arrows still
        // move rather than silently doing nothing.
        let unit = |viewport_span: f32, extent_span: f32| {
            (if viewport_span > 0.0 {
                viewport_span
            } else {
                extent_span
            })
            .max(0.0)
                * fraction
        };
        let (dx, dy) = match step {
            MinimapStep::Leading => (-unit(viewport.width, extent.width), 0.0),
            MinimapStep::Trailing => (unit(viewport.width, extent.width), 0.0),
            MinimapStep::Up => (0.0, -unit(viewport.height, extent.height)),
            MinimapStep::Down => (0.0, unit(viewport.height, extent.height)),
        };
        let centre = viewport.center();
        self.go_to_announcing(Point::new(centre.x + dx, centre.y + dy), ctx);
    }

    /// Centre the view on the content: `content_bounds` unioned with the
    /// items, which is what the picture calls "the scene" — deliberately not
    /// the *effective* extent, which follows the viewport and would make
    /// "go home" a no-op once you had wandered off.
    fn centre_on_content(&self, ctx: &mut EventContext) {
        let home = if rect_is_finite(self.static_extent) {
            self.static_extent
        } else {
            Rect::ZERO
        };
        self.go_to_announcing(home.center(), ctx);
    }
}

impl SceneMinimap {
    /// Construct a minimap that shows at least `content_bounds` (the
    /// scene-coord extent guaranteed to be visible), with `viewport`
    /// driving the live overlay rectangle.
    pub fn new(content_bounds: Rect, viewport: Signal<Rect>) -> Self {
        let size = Size::new(DEFAULT_SIZE.0, DEFAULT_SIZE.1);
        Self {
            content_bounds,
            static_extent: content_bounds,
            viewport_in_scene: viewport,
            items: Vec::new(),
            size,
            painted_projection: Rc::new(Cell::new(Self::seed_projection(content_bounds, size))),
            background: Color::new(1.0, 1.0, 1.0, 0.85),
            border: Some((Color::new(0.0, 0.0, 0.0, 0.5), 1.0)),
            viewport_color: Color::new(0.2, 0.5, 1.0, 1.0),
            content_outline: None,
            on_click: None,
            readout: None,
        }
    }

    /// Override the minimap size. Default `200×150`.
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.size = Size::new(width.max(1.0), height.max(1.0));
        self
    }

    /// Static list of item thumbnails: `(scene_rect, color)`. The
    /// minimap projects each rect onto its drawing area and fills it
    /// with `color`. Apps refresh by rebuilding the widget tree
    /// when items move.
    ///
    /// Item rects widen the projected extent, so a thumbnail outside
    /// a stale or hand-picked `content_bounds` shrinks the picture
    /// instead of painting outside the frame.
    pub fn items(mut self, items: Vec<(Rect, Color)>) -> Self {
        self.static_extent = items
            .iter()
            .fold(self.content_bounds, |acc, (r, _)| union_extent(acc, *r));
        self.items = items;
        self
    }

    /// Background fill color. Default semi-transparent white.
    pub fn background(mut self, color: Color) -> Self {
        self.background = color;
        self
    }

    /// Border around the minimap drawing area. Pass `None` for no
    /// border. Default 1px @ 50% black.
    ///
    /// The band is drawn **inside** the widget — `width` is consumed
    /// from the drawing area, never from the neighbours (the CSS
    /// `border-box` convention). A wide border therefore covers the
    /// outer rim of the picture instead of the sibling next door, and
    /// the width is honoured as given rather than clamped. A width that
    /// is not finite and positive draws nothing.
    pub fn border(mut self, border: Option<(Color, f32)>) -> Self {
        self.border = border;
        self
    }

    /// Color of the viewport overlay rectangle. Default solid blue.
    pub fn viewport_color(mut self, color: Color) -> Self {
        self.viewport_color = color;
        self
    }

    /// Outline the content extent inside the minimap (gives users a
    /// "you're somewhere inside this much scene" cue when the
    /// minimap is taller / wider than its content). Default `None`.
    ///
    /// The outline tracks `content_bounds` *through the projection*,
    /// so once the viewport wanders off it the outline visibly
    /// shrinks and offsets — that is the cue. A width that is not
    /// finite and positive draws nothing.
    pub fn content_outline(mut self, outline: Option<(Color, f32)>) -> Self {
        self.content_outline = outline;
        self
    }

    /// Navigation handler: **centre the view on this scene point.**
    ///
    /// The contract is that specific on purpose. It is not "here is a point,
    /// do as you like": the keyboard and assistive-technology routes compute
    /// their destination from the current viewport and announce where the
    /// move leaves it, and both of those are only true if the app centres.
    ///
    /// `SceneView` has no single "centre on this point" call, so the app
    /// drives the camera it already owns. Either move the view's pan directly
    /// — `SceneView::pan_x_signal` / `pan_y_signal`, or the app-owned pair
    /// handed to `SceneView::view_state` — or, when the view is wrapped in a
    /// `SceneScrollView`, move its `scroll_pos_x_signal` /
    /// `scroll_pos_y_signal`: the same pan through the door the scroll bars
    /// use, already expressed against the scrollable extent so a target can be
    /// clamped to it. `examples/scene_showcase` wires the second.
    ///
    /// This route is the silent one — see this module's "Accessibility" for
    /// which routes speak and why the tap is not among them.
    ///
    /// Installing it is also what turns the minimap from a read-out into a
    /// control: the node becomes focusable and starts advertising its
    /// actions (see this module's "Accessibility"). Without it the widget
    /// still announces where the viewport is, but takes no focus — a Tab
    /// stop with nothing behind it is worse than none.
    ///
    /// A pointer tap inverts the **exact** projection the last paint used —
    /// not a recomputed one — so click-to-recentre lands on what the user is
    /// looking at, even while the viewport is off the content or the parent
    /// handed the minimap less room than it asked for.
    pub fn on_click<F>(mut self, callback: F) -> Self
    where
        F: Fn(Point, &mut EventContext) + 'static,
    {
        self.on_click = Some(Rc::new(callback));
        self
    }

    /// Phrase the accessible readout yourself.
    ///
    /// The closure receives the geometry as a [`MinimapReadout`] and returns
    /// the string the AT node announces as its value and every non-pointer
    /// route announces after a move. Use it to translate the reading
    /// (`tr!(minimap_position(across = …, down = …))`) or to say something
    /// the widget cannot know — page numbers, a chapter name, map
    /// coordinates. The default is English, built by the same numbers.
    ///
    /// One closure for both the value and the announcement on purpose: they
    /// are the same sentence read at two moments, and two hooks would let
    /// them disagree.
    pub fn access_readout<F>(mut self, phrasing: F) -> Self
    where
        F: Fn(MinimapReadout) -> LocalizedString + 'static,
    {
        self.readout = Some(Rc::new(phrasing));
        self
    }

    /// The scene-space rect projected onto the drawing area this
    /// frame: the static extent (content ∪ items) expanded to contain
    /// the live viewport.
    ///
    /// The **only** place this formula lives. `paint` folds it into
    /// [`Self::painted_projection`]; the tap handler reads that
    /// projection rather than re-deriving the extent, so there is no
    /// second copy to drift.
    fn effective_extent(&self) -> Rect {
        union_extent(self.static_extent, self.viewport_in_scene.get())
    }

    /// The string the AT node announces as its value: where the viewport
    /// sits inside the picture, read off the same extent the picture is
    /// projected through so the words and the drawing cannot disagree.
    fn readout_text(&self) -> String {
        let r = readout_of(self.effective_extent(), self.viewport_in_scene.get());
        match &self.readout {
            Some(f) => f(r).resolve_now(),
            None => default_readout_text(r),
        }
    }

    /// The sub-rect of `area` the projection maps onto: inset by half
    /// the viewport indicator's stroke so that indicator is fully
    /// inside the widget even when the viewport *is* the extent.
    fn projection_area(area: Rect) -> Rect {
        let margin = VIEWPORT_STROKE_WIDTH * 0.5;
        area.inset(margin, margin, margin, margin)
    }

    /// The projection `paint` would produce for `extent` in a drawing
    /// area of `size` — used to seed [`Self::painted_projection`] so a
    /// tap arriving before the first paint still lands somewhere sane
    /// instead of on a default transform.
    fn seed_projection(extent: Rect, size: Size) -> MinimapProjection {
        MinimapProjection::fit(
            extent,
            Self::projection_area(Rect::new(0.0, 0.0, size.width, size.height)),
        )
    }
}

impl Widget for SceneMinimap {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // The viewport signal drives our paint output: bind at
        // RepaintOnly so SceneView pan/zoom flips re-render the
        // overlay automatically.
        self.viewport_in_scene.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        // …and again for the AT tree. The announced value IS the viewport
        // position, so a pan that repaints the overlay has to re-walk
        // accessibility too — `RepaintOnly` does not, and the two levels live
        // in separate buckets of the binding registry, so a widget can hold
        // one of each on the same source. Without this the node keeps
        // announcing wherever the viewport was when the minimap was built.
        self.viewport_in_scene.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::AccessibilityOnly,
        );
        // Seed the projection cache so a tap arriving before the first
        // paint still maps against a sane transform.
        self.painted_projection
            .set(Self::seed_projection(self.effective_extent(), self.size));

        if let Some(callback) = self.on_click.clone() {
            // One navigator behind all three routes — see `MinimapNav`.
            let nav = Rc::new(MinimapNav {
                static_extent: self.static_extent,
                viewport: self.viewport_in_scene.clone(),
                readout: self.readout.clone(),
                on_click: callback,
            });
            let painted_projection = Rc::clone(&self.painted_projection);
            let tap_nav = Rc::clone(&nav);
            let key_nav = Rc::clone(&nav);
            let action_nav = Rc::clone(&nav);
            let handlers = HandlerSet::new()
                // Focusable because the tap is a pointer gesture and the
                // widget's only function is behind it (WCAG 2.2 SC 2.1.1).
                // The arrows below are the keyboard route; the actions are
                // the one for a client with no keyboard either.
                .focusable(true)
                .cursor(CursorIcon::Pointer)
                // `on_tap` hands us a widget-local `Point`. Invert the
                // projection `paint` recorded — the same object, not a
                // re-derivation of it — so the click lands on exactly what
                // the user saw. Nothing about the extent or the drawing
                // area is recomputed here, so nothing can drift out of
                // step with paint.
                //
                // `go_to`, not `go_to_announcing`: the user aimed at this
                // point, and click-to-recentre is a gesture people repeat.
                // The node's value carries the new position for anyone who
                // asks for it.
                .on_tap(move |event, ev_ctx| {
                    let scene_pt = painted_projection.get().invert(event.position);
                    tap_nav.go_to(scene_pt, ev_ctx);
                })
                .on_key(move |event, ev_ctx| {
                    let WidgetEvent::KeyDown { key, modifiers, .. } = event else {
                        return EventResponse::Ignored;
                    };
                    // Shift is ours (the page step); every other modifier
                    // belongs to whatever the app bound it to, so a chord
                    // passes through instead of being eaten here.
                    if modifiers.without(Modifiers::SHIFT) != Modifiers::NONE {
                        return EventResponse::Ignored;
                    }
                    if let Some(step) = MinimapStep::from_key(*key) {
                        key_nav.step(step, modifiers.shift(), ev_ctx);
                        return EventResponse::Handled;
                    }
                    if matches!(key, Key::Home | Key::Enter | Key::Space) {
                        key_nav.centre_on_content(ev_ctx);
                        return EventResponse::Handled;
                    }
                    EventResponse::Ignored
                })
                .on_access_action(move |action, ev_ctx| {
                    if let Some(step) = MinimapStep::from_action(action) {
                        // No `Shift` to read: an AT scroll request is one
                        // unit, and the nudge is the safer of the two.
                        action_nav.step(step, false, ev_ctx);
                        return EventResponse::Handled;
                    }
                    if action == Action::Click {
                        action_nav.centre_on_content(ev_ctx);
                        return EventResponse::Handled;
                    }
                    EventResponse::Ignored
                });
            ctx.apply_self_handlers(handlers);
        }
        Vec::new()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // A 2-D position has no ARIA role — there is no `slider2d` — and
        // `numeric_value` holds one number where this reading has four, so
        // the node is a named group carrying its value as text. That is what
        // `HsvCanvas` does for the same reason. It is emphatically NOT a
        // property-less `Role::GenericContainer`: the walker prunes those,
        // and a pruned node announces nothing and advertises nothing.
        builder.set_role(Role::Group);
        builder.set_name(default_access_label().resolve_now());
        builder.set_value(self.readout_text());
        // Actions only where there is something behind them. A minimap with
        // no navigation callback is a read-out: it still says where the
        // viewport is, which is the useful half, but it does not promise an
        // AT client a move it cannot make.
        if self.on_click.is_some() {
            for action in [
                Action::Click,
                Action::ScrollLeft,
                Action::ScrollRight,
                Action::ScrollUp,
                Action::ScrollDown,
            ] {
                builder.add_action(action);
            }
        }
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        let w = proposal
            .width
            .unwrap_or(self.size.width)
            .min(self.size.width);
        let h = proposal
            .height
            .unwrap_or(self.size.height)
            .min(self.size.height);
        Size::new(w, h).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        // `paint` receives absolute bounds (the canvas is NOT pre-translated to
        // the widget origin — unlike `on_tap`, which hands us widget-local
        // coords). The minimap draws everything in its own local frame
        // (origin 0,0), so translate the canvas to `bounds.origin` first — else
        // it renders at the window's top-left regardless of where it's placed.
        canvas.save();
        canvas.translate(bounds.x, bounds.y);
        let area = Rect::new(0.0, 0.0, bounds.width, bounds.height);

        // Everything from here to `clear_clip` — the border included —
        // is bounded by the widget. The extent expansion is what keeps
        // *projected* geometry inside the frame; this clip is the
        // structural backstop that holds even if a future edit
        // regresses that policy, and it is the ONLY one available —
        // `clips_children` does not clip a widget's own paint (see the
        // module docs). Note the order: `set_clip` takes the
        // widget-LOCAL area and must come after the translate above, or
        // it lands at the window origin instead of on the widget.
        canvas.set_clip(area);
        // Background fill: exactly the widget, border included.
        canvas.fill_rect(area, self.background);

        let projection =
            MinimapProjection::fit(self.effective_extent(), Self::projection_area(area));
        // Record the transform `on_tap` inverts. Storing the projection
        // itself — rather than the inputs it was built from — is what
        // makes paint and click structurally incapable of disagreeing.
        self.painted_projection.set(projection);

        // Optional content outline: `content_bounds` through the same
        // projection, so it shrinks away from the frame exactly when
        // the extent grew past it.
        if let Some((color, width)) = self.content_outline
            && rect_is_finite(self.content_bounds)
            && is_drawable_width(width)
        {
            canvas.stroke_rect(
                projection.rect(self.content_bounds),
                color,
                StrokeStyle::solid(width),
            );
        }
        // Item thumbnails.
        for (item_rect, color) in &self.items {
            if !rect_is_finite(*item_rect) {
                continue;
            }
            canvas.fill_rect(projection.rect(*item_rect), *color);
        }
        // Viewport overlay.
        let viewport = self.viewport_in_scene.get();
        if rect_is_finite(viewport) {
            canvas.stroke_rect(
                projection.rect(viewport),
                self.viewport_color,
                StrokeStyle::solid(VIEWPORT_STROKE_WIDTH),
            );
        }
        // Border last so it sits on top of everything — and, unlike
        // every other stroke here, on a rect the caller never supplied:
        // `stroke_rect` centres each edge line on the boundary, so
        // stroking `area` itself would hang `width / 2` outside the
        // widget on all four sides (12 px for a 24 px border, over
        // whatever sibling is under it). Stroking the area inset by
        // half the width puts the band wholly inside instead — see the
        // module docs, "The border is inside the frame".
        if let Some((color, width)) = self.border
            && is_drawable_width(width)
        {
            let half = width * 0.5;
            canvas.stroke_rect(
                area.inset(half, half, half, half),
                color,
                StrokeStyle::solid(width),
            );
        }
        canvas.clear_clip();
        canvas.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_core::widget_tree::WidgetTree;

    fn assert_close(actual: f32, expected: f32, what: &str) {
        assert!(
            (actual - expected).abs() < 1e-3,
            "{what}: expected {expected}, got {actual}"
        );
    }

    #[test]
    fn minimap_default_layout_response_is_capped() {
        let viewport = Signal::new(Rect::new(0.0, 0.0, 100.0, 75.0));
        let mm = SceneMinimap::new(Rect::new(0.0, 0.0, 1000.0, 750.0), viewport);
        let theme = teksilo_core::presets::intui::light();
        let ctx = LayoutContext::for_testing(&theme);
        // Unspecified proposal → falls back to self.size = 200×150.
        let lr = mm.layout_response(SizeProposal::unspecified(), &ctx);
        assert_eq!(lr.size.width, 200.0);
        assert_eq!(lr.size.height, 150.0);
        // Larger proposal → still capped at self.size.
        let lr = mm.layout_response(SizeProposal::exact(400.0, 300.0), &ctx);
        assert_eq!(lr.size.width, 200.0);
        assert_eq!(lr.size.height, 150.0);
    }

    #[test]
    fn minimap_size_override_changes_layout_response() {
        let viewport = Signal::new(Rect::new(0.0, 0.0, 100.0, 75.0));
        let mm = SceneMinimap::new(Rect::new(0.0, 0.0, 1000.0, 750.0), viewport).size(120.0, 80.0);
        let theme = teksilo_core::presets::intui::light();
        let ctx = LayoutContext::for_testing(&theme);
        let lr = mm.layout_response(SizeProposal::unspecified(), &ctx);
        assert_eq!(lr.size.width, 120.0);
        assert_eq!(lr.size.height, 80.0);
        // Caps under a larger proposal.
        let lr = mm.layout_response(SizeProposal::exact(400.0, 300.0), &ctx);
        assert_eq!(lr.size.width, 120.0);
        assert_eq!(lr.size.height, 80.0);
    }

    #[test]
    fn minimap_can_be_added_to_widget_tree() {
        // Smoke test that the widget actually integrates — build()
        // doesn't panic, layout pass succeeds.
        let viewport = Signal::new(Rect::new(0.0, 0.0, 100.0, 75.0));
        let mm = SceneMinimap::new(Rect::new(0.0, 0.0, 1000.0, 750.0), viewport).size(120.0, 80.0);
        let mut tree = WidgetTree::new();
        let id = tree.add(mm);
        tree.layout(SizeProposal::unspecified());
        let bounds = tree.bounds(id);
        // With unspecified proposal, the framework respects layout_response.
        assert_eq!(bounds.width, 120.0);
        assert_eq!(bounds.height, 80.0);
    }

    #[test]
    fn projection_maps_extent_corners_onto_the_drawing_area() {
        // Matched aspect (4:3 into 4:3) — the fit uses the whole area.
        let extent = Rect::new(0.0, 0.0, 1000.0, 750.0);
        let area = Rect::new(0.0, 0.0, 200.0, 150.0);
        let p = MinimapProjection::fit(extent, area);
        let tl = p.point(Point::new(0.0, 0.0));
        assert_close(tl.x, 0.0, "top-left x");
        assert_close(tl.y, 0.0, "top-left y");
        let br = p.point(Point::new(1000.0, 750.0));
        assert_close(br.x, 200.0, "bottom-right x");
        assert_close(br.y, 150.0, "bottom-right y");
        let centre = p.point(Point::new(500.0, 375.0));
        assert_close(centre.x, 100.0, "centre x");
        assert_close(centre.y, 75.0, "centre y");
    }

    #[test]
    fn projection_invert_is_the_exact_inverse() {
        let extent = Rect::new(50.0, 100.0, 1000.0, 750.0);
        let area = Rect::new(0.0, 0.0, 200.0, 150.0);
        let p = MinimapProjection::fit(extent, area);
        for (sx, sy) in [
            (50.0, 100.0),
            (1050.0, 850.0),
            (550.0, 475.0),
            (-400.0, 9.0),
        ] {
            let back = p.invert(p.point(Point::new(sx, sy)));
            assert_close(back.x, sx, "round-trip x");
            assert_close(back.y, sy, "round-trip y");
        }
    }

    #[test]
    fn projection_is_uniform_and_centres_the_letterbox() {
        // A 2:1 extent in a square area: the fit is limited by width,
        // and the picture is centred vertically.
        let extent = Rect::new(0.0, 0.0, 1000.0, 500.0);
        let area = Rect::new(0.0, 0.0, 200.0, 200.0);
        let p = MinimapProjection::fit(extent, area);
        assert_close(p.scale, 0.2, "scale is the tighter of the two axes");
        let tl = p.point(Point::new(0.0, 0.0));
        assert_close(tl.x, 0.0, "no horizontal letterbox");
        assert_close(tl.y, 50.0, "vertical letterbox is half the slack");
        let br = p.point(Point::new(1000.0, 500.0));
        assert_close(br.x, 200.0, "fills the width");
        assert_close(br.y, 150.0, "and stops short of the height");
        // A square scene rect stays square on the minimap.
        let square = p.rect(Rect::new(100.0, 100.0, 200.0, 200.0));
        assert_close(square.width, square.height, "aspect preserved");
    }

    #[test]
    fn projection_of_a_degenerate_extent_is_finite_and_centred() {
        let area = Rect::new(0.0, 0.0, 200.0, 150.0);
        // Wholly degenerate (an empty scene at rest): 1:1 about the centre.
        let p = MinimapProjection::fit(Rect::ZERO, area);
        assert_close(p.scale, 1.0, "degenerate extent falls back to 1:1");
        let origin = p.point(Point::ZERO);
        assert_close(origin.x, 100.0, "origin lands at the area centre x");
        assert_close(origin.y, 75.0, "origin lands at the area centre y");
        // One degenerate axis: the other still sets the scale.
        let p = MinimapProjection::fit(Rect::new(0.0, 0.0, 400.0, 0.0), area);
        assert_close(p.scale, 0.5, "the live axis sets the scale");
        let mid = p.point(Point::new(200.0, 0.0));
        assert_close(mid.x, 100.0, "centred horizontally");
        assert_close(mid.y, 75.0, "the flat axis collapses to the centre");
    }

    #[test]
    fn projection_of_a_non_finite_extent_stays_finite() {
        let area = Rect::new(0.0, 0.0, 200.0, 150.0);
        for bad in [
            Rect::new(f32::NAN, 0.0, 10.0, 10.0),
            Rect::new(0.0, 0.0, f32::INFINITY, 10.0),
        ] {
            let p = MinimapProjection::fit(bad, area);
            assert!(p.scale.is_finite() && p.scale > 0.0, "scale stays usable");
            let q = p.point(Point::new(1.0, 1.0));
            assert!(q.x.is_finite() && q.y.is_finite(), "output stays finite");
        }
    }

    #[test]
    fn projection_rect_floors_at_one_pixel() {
        let p = MinimapProjection::fit(
            Rect::new(0.0, 0.0, 10_000.0, 10_000.0),
            Rect::new(0.0, 0.0, 100.0, 100.0),
        );
        // A 1-unit item at a 0.01 scale would be 0.01 px — still drawn.
        let r = p.rect(Rect::new(0.0, 0.0, 1.0, 1.0));
        assert_close(r.width, 1.0, "floored width");
        assert_close(r.height, 1.0, "floored height");
    }

    #[test]
    fn effective_extent_expands_to_contain_the_live_viewport() {
        let content = Rect::new(0.0, 0.0, 1000.0, 1000.0);
        let viewport = Signal::new(Rect::new(3000.0, 3000.0, 200.0, 200.0));
        let mm = SceneMinimap::new(content, viewport.clone());
        assert_eq!(mm.effective_extent(), Rect::new(0.0, 0.0, 3200.0, 3200.0));
        // …and it follows the signal, with no rebuild.
        viewport.set(Rect::new(-500.0, -500.0, 100.0, 100.0));
        assert_eq!(
            mm.effective_extent(),
            Rect::new(-500.0, -500.0, 1500.0, 1500.0)
        );
    }

    #[test]
    fn effective_extent_expands_to_contain_items_outside_content_bounds() {
        let content = Rect::new(0.0, 0.0, 100.0, 100.0);
        let viewport = Signal::new(Rect::new(0.0, 0.0, 100.0, 100.0));
        let mm = SceneMinimap::new(content, viewport).items(vec![
            (Rect::new(500.0, 20.0, 50.0, 50.0), Color::RED),
            (Rect::new(-60.0, -10.0, 10.0, 10.0), Color::BLUE),
        ]);
        assert_eq!(mm.static_extent, Rect::new(-60.0, -10.0, 610.0, 110.0));
        assert_eq!(mm.effective_extent(), Rect::new(-60.0, -10.0, 610.0, 110.0));
    }

    #[test]
    fn effective_extent_ignores_non_finite_inputs() {
        let content = Rect::new(0.0, 0.0, 100.0, 100.0);
        let viewport = Signal::new(Rect::new(f32::NAN, 0.0, 10.0, 10.0));
        let mm = SceneMinimap::new(content, viewport).items(vec![(
            Rect::new(0.0, f32::INFINITY, 10.0, 10.0),
            Color::RED,
        )]);
        assert_eq!(mm.static_extent, content, "a non-finite item is skipped");
        assert_eq!(
            mm.effective_extent(),
            content,
            "a non-finite viewport is skipped"
        );
    }

    #[test]
    fn projection_area_never_goes_negative() {
        // A 1×1 widget: the inset would take the area past zero.
        let a = SceneMinimap::projection_area(Rect::new(0.0, 0.0, 1.0, 1.0));
        assert!(a.width >= 0.0 && a.height >= 0.0);
        // …and the projection built from it is still usable.
        let p = MinimapProjection::fit(Rect::new(0.0, 0.0, 10.0, 10.0), a);
        assert!(p.scale.is_finite() && p.scale > 0.0);
    }

    #[test]
    fn readout_reports_position_and_coverage_as_fractions() {
        let r = readout_of(
            Rect::new(0.0, 0.0, 1000.0, 500.0),
            Rect::new(400.0, 0.0, 200.0, 250.0),
        );
        assert_close(r.position.x, 0.5, "centred horizontally");
        assert_close(r.position.y, 0.25, "top quarter vertically");
        assert_close(r.coverage.width, 0.2, "a fifth of the width");
        assert_close(r.coverage.height, 0.5, "half the height");
    }

    #[test]
    fn readout_of_a_degenerate_or_non_finite_input_is_still_a_reading() {
        // A degenerate axis is "dead centre, seeing all of it" rather than a
        // division by zero — an empty scene must not announce NaN.
        let r = readout_of(Rect::ZERO, Rect::ZERO);
        assert_close(r.position.x, 0.5, "degenerate x position");
        assert_close(r.coverage.height, 1.0, "degenerate y coverage");
        // A non-finite viewport falls back to the extent: you are looking at
        // all of it.
        let extent = Rect::new(10.0, 10.0, 100.0, 100.0);
        let r = readout_of(extent, Rect::new(f32::NAN, 0.0, 10.0, 10.0));
        assert_eq!(r.viewport, extent);
        assert_close(r.coverage.width, 1.0, "fallback coverage");
        // …and a non-finite extent collapses to zero rather than propagating.
        let r = readout_of(Rect::new(0.0, f32::INFINITY, 1.0, 1.0), Rect::ZERO);
        assert_eq!(r.extent, Rect::ZERO);
        assert!(r.position.x.is_finite() && r.coverage.width.is_finite());
    }

    #[test]
    fn a_viewport_outside_the_content_still_reads_inside_the_picture() {
        // The extent expands to contain the viewport, so the fractions are
        // bounded by construction — the words cannot describe a position the
        // picture does not draw.
        let mm = SceneMinimap::new(
            Rect::new(0.0, 0.0, 1000.0, 1000.0),
            Signal::new(Rect::new(5000.0, -5000.0, 100.0, 100.0)),
        );
        let r = readout_of(mm.effective_extent(), mm.viewport_in_scene.get());
        assert!((0.0..=1.0).contains(&r.position.x));
        assert!((0.0..=1.0).contains(&r.position.y));
        assert!((0.0..=1.0).contains(&r.coverage.width));
    }

    #[test]
    fn the_default_readout_text_rounds_to_whole_percents() {
        let text = default_readout_text(readout_of(
            Rect::new(0.0, 0.0, 1000.0, 1000.0),
            Rect::new(400.0, 400.0, 200.0, 200.0),
        ));
        assert_eq!(
            text,
            "Viewport at 50% across, 50% down; showing 20% of the width and 20% of the height"
        );
    }

    #[test]
    fn items_builder_recomputes_the_static_extent_from_scratch() {
        // Calling `.items()` twice must not accumulate the first call's
        // rects into the extent — it replaces them.
        let content = Rect::new(0.0, 0.0, 100.0, 100.0);
        let viewport = Signal::new(Rect::ZERO);
        let mm = SceneMinimap::new(content, viewport)
            .items(vec![(Rect::new(900.0, 900.0, 10.0, 10.0), Color::RED)])
            .items(vec![(Rect::new(50.0, 50.0, 10.0, 10.0), Color::BLUE)]);
        assert_eq!(mm.static_extent, content);
    }
}
