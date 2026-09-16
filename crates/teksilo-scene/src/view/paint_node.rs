// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The two paint positions a `SceneView` cannot reach from its own `paint`.
//!
//! The render walker gives a widget exactly two places to draw: before its
//! child subtree (`paint`) and after it (`post_paint`). Those are the
//! [`Under`](crate::SceneLayer::Under) and [`Over`](crate::SceneLayer::Over)
//! bands, and between them sits the whole heavyweight tier as one block. Two
//! things need a position *inside* that block:
//!
//! - **[`SceneLayer::Interleaved`](crate::SceneLayer::Interleaved)** — a
//!   lightweight item ordered against the cards by `z`, so a stroke can be
//!   above note A and below note B. [`SceneBandProxy`] is the node that carries
//!   one, slotted into the child list by [`PaintKey`].
//! - **A wet surface** — content being authored right now, which must repaint
//!   on its own without dragging the item bands through a pass with it.
//!   [`WetLayer`] is the handle; [`WetLayerNode`] is its node.
//!
//! Both nodes are **paint only**, and that is what makes them affordable:
//!
//! | | consequence |
//! |---|---|
//! | `event_pass_through` | the pointer resolves through the lightweight hit snapshot exactly as before — one picker, still |
//! | left out of `accessibility_children` | the AT tree is bit-identical; an interleaved item keeps the same synthetic node it had in `Under` |
//! | not focusable, no handlers | not a Tab stop, so `tab_stops_within` is unchanged |
//!
//! # Why there is a bridge rather than a `&SceneView`
//!
//! A child node cannot borrow its parent widget. Everything the per-item paint
//! loop reads is already a cloneable handle on the view — the model, the
//! item cache, the four camera signals, the drag cell, the transform driver —
//! so [`ScenePaintBridge`] is those handles in a bundle, and
//! [`ScenePaintBridge::paint_items`] is the loop, called by the view's own
//! `paint_band` **and** by these nodes. One implementation, so an interleaved
//! item cannot render differently from the same item in `Under`.
//!
//! # Layer order
//!
//! Stated here because it is otherwise implicit in three files:
//!
//! 1. `SceneView::paint` — the app background closure, then the `Under` band
//! 2. the arena's child walk, sorted by [`PaintKey`]:
//!    [`SceneBandProxy`] nodes and heavyweight cards interleaved by `z`
//! 3. [`WetLayerNode`] — always the last child, so wet content sits above
//!    every card and every interleaved item
//! 4. `SceneView::post_paint` — the `Over` band, the selection marquee, the app
//!    foreground closure, magnetism feedback, the transform chrome, the debug
//!    overlay
//!
//! So a wet stroke is drawn over the content it is being drawn *on*, and under
//! the view's own chrome — a marquee, a magnet ghost and the debug overlay stay
//! visible through it. An app whose dried ink lives in the `Over` band will see
//! the stroke rise one step at the moment it dries; `Interleaved` (or `Under`)
//! has no such step, which is the other reason ink belongs in a band of its
//! own.

use super::*;
use crate::item::{ItemId, SceneItemPaintContext};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use teksilo_canvas::{Canvas, Point, Rect, Transform2D};

/// Everything the per-item paint loop reads, as clonable handles.
///
/// Built by [`SceneView::paint_bridge`] — about a dozen refcount bumps, which
/// is nothing beside the loop it serves — and held for the node's lifetime by
/// [`SceneBandProxy`] and [`WetLayerNode`].
#[derive(Clone)]
pub(crate) struct ScenePaintBridge {
    model: SceneModel,
    item_cache: Rc<RefCell<crate::cache::ItemCoordinateCache>>,
    pan_x: Signal<f32>,
    pan_y: Signal<f32>,
    zoom: Signal<f32>,
    rotation: Signal<f32>,
    bounds_origin: Signal<Vec2>,
    drag_target: Rc<Cell<Option<DragTarget>>>,
    selection: crate::selection::SceneSelection,
    transform: Option<super::transform::TransformDriver>,
    /// The view's own visible scene region, republished every `place_children`
    /// and every `SceneView::paint`.
    ///
    /// A child node has no way to derive it — it knows its own scene rect, not
    /// the viewport's — and an item reads it as a hint to skip geometry it
    /// cannot show (a tiled grid is the case). Publishing rather than deriving
    /// is safe in both directions: a pan is a relayout, so `place_children`
    /// refreshes it before any node paints; and when nothing moved, last
    /// pass's value is this pass's value.
    visible_region: Rc<Cell<Rect>>,
}

impl std::fmt::Debug for ScenePaintBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScenePaintBridge").finish_non_exhaustive()
    }
}

impl ScenePaintBridge {
    /// The composed view transform, computed from the same four signals
    /// [`SceneView::view_transform`] reads — not snapshotted, so a node painted
    /// out of step with the view still projects correctly.
    pub(crate) fn view_transform(&self) -> Transform2D {
        let bo = self.bounds_origin.get();
        super::compose_view(
            Vec2::new(self.pan_x.get() + bo.x, self.pan_y.get() + bo.y),
            self.zoom.get(),
            self.rotation.get(),
        )
    }

    /// The viewport in scene coordinates, as the view last published it.
    pub(crate) fn visible_region(&self) -> Rect {
        self.visible_region.get()
    }

    /// Paint `ids`, in the order given, under the active view transform.
    ///
    /// Callers sort. This is the whole of the lightweight tier's per-item
    /// rendering — the drag and transform previews, `IGNORES_TRANSFORMATIONS`
    /// pinning, effective opacity, the item-coordinate frame cache and its
    /// raster-scale refinement — and it is the only copy of it.
    pub(crate) fn paint_items(
        &self,
        ids: &[ItemId],
        canvas: &mut Canvas,
        region: Rect,
        ctx: &PaintContext,
    ) {
        if ids.is_empty() {
            return;
        }
        // Glyph-epoch gate: cached item frames bake glyph atlas UVs, and this
        // cache lives outside the widget arena, so the framework's eviction
        // recovery (`invalidate_all_paints`) cannot reach it. Instead, every
        // paint pass compares the backend's eviction epoch and drops all
        // entries when it moved — the items repaint below with fresh UVs in the
        // same pass.
        let glyph_epoch = canvas
            .text_backend()
            .map(|tb| tb.borrow().glyph_epoch())
            .unwrap_or(0);
        self.item_cache.borrow_mut().sync_glyph_epoch(glyph_epoch);

        // Ambient text raster scale, set by the paint walker for this
        // SceneView's content-transform scope (it already includes the view's
        // zoom). Items whose own pushed transform carries an additional scale
        // refine it below so their text rasterizes at the full effective
        // density.
        let ambient_raster_scale = canvas
            .text_backend()
            .map(|tb| tb.borrow().raster_scale())
            .unwrap_or(1.0);

        let view_transform = self.view_transform();
        let mut item_ctx = SceneItemPaintContext::new(view_transform, Some(region), ctx.theme)
            .with_text_scale(ctx.text_scale)
            .with_window_active(ctx.window_active);

        let drag_target = self.drag_target.get();
        // Every item the in-flight drag carries — the grabbed one alone, or the
        // whole selection when the grab landed on a selected item. Resolved
        // once per pass rather than per item, and from the same
        // `drag_group_of` the commit uses, so the live feedback and the
        // committed move can never show different sets.
        let drag_group: Vec<ItemId> = drag_target
            .map(|t| super::drag_group_of(&self.model, &self.selection, t.item_id))
            .unwrap_or_default();
        // The selection transform's live preview. The heavyweight tier reads
        // the SAME function in `place_children` — one affine, two tiers, so a
        // mixed selection cannot half-move mid-gesture.
        let transform_preview = self
            .transform
            .as_ref()
            .filter(|d| d.is_enabled())
            .and_then(|d| d.preview());

        for &id in ids {
            let scene = self.model.0.borrow();
            if scene.item(id).is_none() {
                continue;
            }
            if !scene.is_effectively_visible(id) {
                continue;
            }
            let flags = scene.flags(id).unwrap_or_default();
            if flags.contains(crate::flags::ItemFlags::HAS_NO_CONTENTS) {
                continue;
            }
            // Per-item enabled state drives `ColorProp` disabled-role
            // resolution. AND-combine the item's own `IS_ENABLED` flag with the
            // widget-tree's ancestor-disabled cascade, so a SceneView inside a
            // disabled ancestor dims its lightweight items' role colours
            // exactly like every other widget in that subtree.
            item_ctx.enabled =
                ctx.effective_enabled && flags.contains(crate::flags::ItemFlags::IS_ENABLED);

            // Items that are the drag target or a declared descendant paint
            // with a visual delta in scene coords — a child follows its dragged
            // parent until the rebuild commits the new local_pos.
            let drag_delta = drag_target
                .filter(|_| {
                    drag_group
                        .iter()
                        .any(|g| *g == id || scene.is_descendant_of(id, *g))
                })
                .map(|t| {
                    Transform2D::translate(
                        t.current_scene.x - t.anchor_scene.x,
                        t.current_scene.y - t.anchor_scene.y,
                    )
                });

            // …and the transform controller's preview, on the roots it carries
            // and everything hanging off them.
            let transform_delta = transform_preview.as_ref().and_then(|(roots, xform)| {
                roots
                    .iter()
                    .any(|r| *r == id || scene.is_descendant_of(id, *r))
                    .then_some(*xform)
            });

            let mut local_to_scene = scene.scene_transform(id);
            if let Some(t) = drag_delta {
                local_to_scene = local_to_scene.then(&t);
            }
            if let Some(t) = transform_delta {
                local_to_scene = local_to_scene.then(&t);
            }
            canvas.save();
            // IGNORES_TRANSFORMATIONS items pin at their parent-relative
            // position but render at a fixed pixel size (Qt's
            // `ItemIgnoresTransformations`).
            let pushed_transform =
                if flags.contains(crate::flags::ItemFlags::IGNORES_TRANSFORMATIONS) {
                    let scene_anchor = local_to_scene.apply_point(Point::ZERO);
                    let screen_anchor = view_transform.apply_point(scene_anchor);
                    let view_inv = view_transform
                        .inverse()
                        .unwrap_or_else(Transform2D::identity);
                    Transform2D::translate(screen_anchor.x, screen_anchor.y).then(&view_inv)
                } else {
                    local_to_scene
                };
            canvas.apply_transform(pushed_transform);
            let item_raster_scale = teksilo_canvas::quantize_raster_scale(
                ambient_raster_scale * pushed_transform.geometric_scale(),
            );
            let raster_scale_changed = item_raster_scale != ambient_raster_scale;
            if raster_scale_changed && let Some(tb) = canvas.text_backend() {
                tb.borrow_mut().set_raster_scale(item_raster_scale);
            }
            let alpha = scene.effective_opacity(id);
            let opacity_pushed = alpha < 0.999;
            if opacity_pushed {
                canvas.set_opacity(alpha);
            }
            if let Some(item) = scene.item(id) {
                match item.cache_mode() {
                    crate::cache::CacheMode::ItemCoordinate => {
                        let cached = self.item_cache.borrow().get(id, item_raster_scale).cloned();
                        if let Some(frame) = cached {
                            canvas.draw_render_frame(&frame, Point::ZERO);
                        } else {
                            let mut sub = match canvas.text_backend() {
                                Some(tb) => Canvas::with_text_backend(tb.clone()),
                                None => Canvas::new(),
                            };
                            item.paint(&mut sub, &item_ctx);
                            let frame = sub.into_render_frame();
                            canvas.draw_render_frame(&frame, Point::ZERO);
                            self.item_cache
                                .borrow_mut()
                                .insert(id, frame, item_raster_scale);
                        }
                    }
                    crate::cache::CacheMode::None => {
                        item.paint(canvas, &item_ctx);
                    }
                }
            }
            if raster_scale_changed && let Some(tb) = canvas.text_backend() {
                tb.borrow_mut().set_raster_scale(ambient_raster_scale);
            }
            if opacity_pushed {
                canvas.restore_opacity();
            }
            canvas.restore();
        }
    }
}

impl SceneView {
    /// The handles a child paint node needs. See [`ScenePaintBridge`].
    pub(crate) fn paint_bridge(&self) -> ScenePaintBridge {
        ScenePaintBridge {
            model: self.model.clone(),
            item_cache: self.item_cache.clone(),
            pan_x: self.pan_x.clone(),
            pan_y: self.pan_y.clone(),
            zoom: self.zoom.clone(),
            rotation: self.rotation.clone(),
            bounds_origin: self.bounds_origin_signal.clone(),
            drag_target: self.drag_target.clone(),
            selection: self.selection.clone(),
            transform: self.transform_driver(),
            visible_region: self.published_visible_region.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Interleaved band
// ---------------------------------------------------------------------------

/// The node that paints one [`SceneLayer::Interleaved`](crate::SceneLayer)
/// item, so it can sit between two cards.
///
/// Paint only — see the module docs for the three properties that makes true
/// and why they are what make the band affordable.
pub(crate) struct SceneBandProxy {
    bridge: ScenePaintBridge,
    item: ItemId,
}

impl std::fmt::Debug for SceneBandProxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneBandProxy")
            .field("item", &self.item)
            .finish()
    }
}

impl SceneBandProxy {
    pub(crate) fn new(bridge: ScenePaintBridge, item: ItemId) -> Self {
        Self { bridge, item }
    }
}

impl Widget for SceneBandProxy {
    /// Zero, because the view writes this node's placement outright in
    /// `place_children` — the item's own scene rect. Returning the item's size
    /// here as well would be a second answer for the same question, and the
    /// one the parent overrides.
    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::ZERO.into()
    }

    fn paint(&self, _bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        // `bounds` is deliberately unused: the item is painted at its own scene
        // coordinates through `scene_transform`, exactly as it would be in
        // `Under`, and this node's rect exists only to place it in the child
        // list and to tell the walker roughly where it is. Painting relative to
        // `bounds` would apply the position twice.
        let region = self.bridge.visible_region();
        self.bridge.paint_items(&[self.item], canvas, region, ctx);
    }

    /// Nothing. The item's AT node is the synthetic one the scene's own walker
    /// emits — banding must not change what assistive technology is told.
    fn accessibility(&self, builder: &mut teksilo_core::accessibility::AccessNodeBuilder) {
        builder.set_hidden();
    }
}

// ---------------------------------------------------------------------------
// Wet layer
// ---------------------------------------------------------------------------

/// A surface for content being authored **right now**, which repaints without
/// taking the scene's item bands with it.
///
/// The case is ink. A stroke in flight changes on every pointer sample, and
/// there is no way to invalidate a `SceneView`'s foreground alone: the render
/// walker computes **one** `needs_paint` per node and gates that node's `paint`
/// and its `post_paint` with it, so asking the view to repaint re-runs the
/// whole `Under` band as well. A wet layer is its own node, so
/// [`request_repaint`](Self::request_repaint) marks that node and nothing else.
///
/// The painter draws in **scene** coordinates — the node lives inside the
/// view's content transform, so a wet stroke pans and zooms with the page for
/// free, and a scene point can be drawn at its own value.
///
/// Where it lands in the paint order, and why: above every card and every
/// [`Interleaved`](crate::SceneLayer::Interleaved) item, under the `Over` band
/// and the view's own chrome (marquee, magnet feedback, transform frame, debug
/// overlay). `docs/teksilo-scene.md` states the whole order.
///
/// ```no_run
/// # use std::cell::RefCell;
/// # use std::rc::Rc;
/// # use teksilo_scene::{Scene, SceneView, WetLayer};
/// # use teksilo_canvas::Point;
/// # use teksilo_tokens::Color;
/// let points: Rc<RefCell<Vec<Point>>> = Rc::default();
/// let wet = {
///     let points = points.clone();
///     WetLayer::new(move |canvas, _ctx| {
///         for p in points.borrow().iter() {
///             canvas.fill_circle(*p, 2.0, Color::BLACK);
///         }
///     })
/// };
/// let view = SceneView::new(Scene::new()).wet_layer(wet.clone());
/// // …and from a pointer handler, after pushing a point:
/// // wet.request_repaint(ctx);
/// ```
///
/// A cloneable handle: clone it into the pointer handler that feeds it and into
/// [`SceneView::wet_layer`], the way a [`SceneModel`] is cloned into its views.
///
/// # More than one view
///
/// The comparison with [`SceneModel`] is meant literally. Mount the same layer
/// in two views — an overview beside a detail pane — and **both** paint it, and
/// [`request_repaint`](Self::request_repaint) repaints both. A second view does
/// not displace the first.
///
/// One thing does not carry across, and it is a property of the door rather
/// than of this type: a repaint is requested through an
/// [`EventContext`](teksilo_core::EventContext), and a context belongs to one
/// window's tree. So `request_repaint` repaints the mounts **in the calling
/// context's own window** and leaves a mount in another window to that window's
/// own frame — an id from the other tree could not reach it anyway, because
/// ids are per-arena slot keys and two trees mint the same ones.
///
/// That last sentence is the reason `request_repaint` does not simply push the
/// ids it holds: it resolves each one in the calling tree through a typed door
/// first, so a stale id can only ever land on one of this layer's own nodes
/// there — or trip an assertion. The rule and its one remaining edge (the same
/// layer mounted in two **window-less** trees) are written out at
/// [`request_repaint`](Self::request_repaint).
#[derive(Clone)]
pub struct WetLayer(Rc<WetLayerInner>);

struct WetLayerInner {
    painter: Box<dyn Fn(&mut Canvas, &SceneItemPaintContext<'_>)>,
    /// One entry per view that has mounted this layer.
    ///
    /// `Weak`, and the strong [`WetMount`] is owned by the view — so a view
    /// that is destroyed takes its entry out of circulation without needing a
    /// `Drop` impl on [`SceneView`] (which would forbid the by-value builder
    /// methods from moving fields out of `self`).
    mounts: RefCell<Vec<Weak<WetMount>>>,
}

/// One view's mount of a [`WetLayer`]: the node it paints into, and the window
/// whose tree that node belongs to.
///
/// Held strongly by the [`SceneView`] that minted it and weakly by the layer,
/// so its lifetime is the view's.
pub(crate) struct WetMount {
    node: WidgetId,
    /// `None` for a tree with no window — a headless test. Two window-less
    /// trees wear the same `None`, so this narrows the candidates and does not
    /// on its own decide them; what makes the marking safe anyway is the
    /// **typed** door [`WetLayer::request_repaint`] pushes ids through. See its
    /// own doc.
    window: Option<teksilo_core::window::TeksiloWindowId>,
}

/// One of a [`WetLayer`]'s mounted paint nodes, as
/// [`WetLayer::nodes`] reports it.
///
/// A bare [`WidgetId`] is a slot key in *some* arena and says which one it is
/// not: two trees mint the same ids, so a caller holding several trees cannot
/// tell whose node it has. This pairs the id with the window whose tree it
/// belongs to, which is the identity the framework has — `None` for a headless
/// tree, where the caller owns the question and the count is the warning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct WetNode {
    id: WidgetId,
    window: Option<teksilo_core::window::TeksiloWindowId>,
}

impl WetNode {
    /// A node report. The framework builds these; the constructor exists so a
    /// consumer can build one in a test of its own.
    pub fn new(id: WidgetId, window: Option<teksilo_core::window::TeksiloWindowId>) -> Self {
        Self { id, window }
    }

    /// The paint node, in the arena of the window below.
    pub fn id(&self) -> WidgetId {
        self.id
    }

    /// The window whose tree holds [`id`](Self::id), or `None` for a tree that
    /// has no window.
    pub fn window(&self) -> Option<teksilo_core::window::TeksiloWindowId> {
        self.window
    }
}

impl std::fmt::Debug for WetLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WetLayer")
            .field("nodes", &self.nodes())
            .finish_non_exhaustive()
    }
}

impl WetLayer {
    /// A wet surface painted by `painter`, in **scene** coordinates.
    ///
    /// The closure is called on every repaint of a node this layer is mounted
    /// in, and on nothing else. It owns whatever the app is authoring — a
    /// stroke's points, a rubber-band shape, a live measurement — typically
    /// through an `Rc<RefCell<…>>` shared with the pointer handler that feeds
    /// it.
    pub fn new(painter: impl Fn(&mut Canvas, &SceneItemPaintContext<'_>) + 'static) -> Self {
        Self(Rc::new(WetLayerInner {
            painter: Box::new(painter),
            mounts: RefCell::new(Vec::new()),
        }))
    }

    /// Repaint the wet surface, and only it.
    ///
    /// No relayout, no rebuild, no accessibility re-walk — and, crucially, no
    /// repaint of the scene's item bands: this marks nodes, one per view that
    /// mounted the layer in `ctx`'s own window.
    ///
    /// A no-op until the layer has been mounted through
    /// [`SceneView::wet_layer`] and the view has built, which is the frame
    /// before any handler can run.
    ///
    /// # Why the id goes through a typed door
    ///
    /// A [`WidgetId`] is a slot key in *some* arena, and every tree mints the
    /// same ones. The window a mount was made in narrows the candidates — a
    /// tree belongs to exactly one window — but a headless tree has no window,
    /// so `None == None` is not a match, it is two unknowns comparing equal.
    /// Marking on that alone is how a repaint from one test's tree lands on
    /// whatever unrelated widget happens to hold that slot in another's.
    ///
    /// So the mark is not pushed as a bare
    /// [`request_repaint`](teksilo_core::EventContext::request_repaint). It
    /// goes through
    /// [`with_widget_mut`](teksilo_core::EventContext::with_widget_mut), which
    /// resolves the id **in the calling tree** and hands the node over only if
    /// it downcasts to this crate's own `WetLayerNode` — and the closure then
    /// checks it is a node of *this* layer. Both checks fail loudly
    /// (`debug_assert`) rather
    /// than quietly, so a cross-tree repaint is a test failure instead of a
    /// stray dirty bit.
    ///
    /// What that buys, stated exactly:
    ///
    /// - An id naming a slot the calling tree does not fill, or fills with a
    ///   widget that has not opted into `Widget::as_any_mut` — which is every
    ///   ordinary widget, the default being `None` — reaches **nothing**, in
    ///   both profiles. That is the case this used to get wrong: the foreign id
    ///   was marked, and whatever held the slot repainted.
    /// - An id naming a `WetLayerNode` of *this* layer is marked, which is
    ///   right: that node's own mount is in the same list, so it wanted
    ///   repainting anyway.
    /// - An id naming a node that *has* opted into `as_any_mut` and is not one
    ///   of this layer's trips the assertion, so in a test or a dev build it is
    ///   a panic. In release the closure is skipped and the node still takes the
    ///   `RepaintOnly` mark, which costs one repaint of something already on
    ///   screen — never a mutation, never a relayout, rebuild or accessibility
    ///   change.
    ///
    /// So this does **not** make one layer in two window-less trees work; it
    /// makes the attempt fail where it can be seen. Drive each tree's repaint
    /// itself, from [`nodes`](Self::nodes).
    pub fn request_repaint(&self, ctx: &mut teksilo_core::EventContext<'_>) {
        let here = ctx.window().map(|w| w.id());
        for mount in self.live_mounts() {
            if mount.window != here {
                continue;
            }
            let layer = self.clone();
            ctx.with_widget_mut::<WetLayerNode>(
                mount.node,
                teksilo_core::binding::BindingLevel::RepaintOnly,
                move |node| {
                    debug_assert!(
                        node.belongs_to(&layer),
                        "WetLayer::request_repaint: the node is a wet surface, but \
                         another layer's — the id came from a mount in a different \
                         tree. Drive that tree's repaint from `WetLayer::nodes`."
                    );
                },
            );
        }
    }

    /// The nodes this layer paints into — one per mounted view, in mount order,
    /// each paired with the window whose tree holds it.
    ///
    /// Exposed for a caller that wants to drive the repaint through some other
    /// door than [`request_repaint`](Self::request_repaint) — a
    /// `WidgetTree::mark_needs_paint` in a test, say. Such a caller owns the
    /// question `request_repaint` answers from its context: **which tree** a
    /// given node belongs to, which is why this reports
    /// [`WetNode::window`] beside the id rather than the id alone. With one
    /// view (the overwhelmingly common case) that is the one tree there is.
    pub fn nodes(&self) -> Vec<WetNode> {
        self.live_mounts()
            .into_iter()
            .map(|m| WetNode::new(m.node, m.window))
            .collect()
    }

    /// Mount this layer into `node`, in the tree of `window`. Returns the
    /// strong handle the view holds for as long as it lives.
    pub(crate) fn attach(
        &self,
        node: WidgetId,
        window: Option<teksilo_core::window::TeksiloWindowId>,
    ) -> Rc<WetMount> {
        let mount = Rc::new(WetMount { node, window });
        let mut mounts = self.0.mounts.borrow_mut();
        mounts.retain(|m| m.strong_count() > 0);
        mounts.push(Rc::downgrade(&mount));
        mount
    }

    /// The mounts whose view is still alive, dropping the rest on the way past.
    fn live_mounts(&self) -> Vec<Rc<WetMount>> {
        let mut mounts = self.0.mounts.borrow_mut();
        mounts.retain(|m| m.strong_count() > 0);
        mounts.iter().filter_map(Weak::upgrade).collect()
    }

    fn paint_with(&self, canvas: &mut Canvas, ctx: &SceneItemPaintContext<'_>) {
        (self.0.painter)(canvas, ctx);
    }
}

/// The node [`WetLayer`] paints into. Always the last child of its view.
pub(crate) struct WetLayerNode {
    bridge: ScenePaintBridge,
    layer: WetLayer,
}

impl std::fmt::Debug for WetLayerNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WetLayerNode").finish_non_exhaustive()
    }
}

impl WetLayerNode {
    pub(crate) fn new(bridge: ScenePaintBridge, layer: WetLayer) -> Self {
        Self { bridge, layer }
    }

    /// Whether this node paints `layer` — the identity check
    /// [`WetLayer::request_repaint`] runs once the arena has resolved an id in
    /// the calling tree. Pointer equality on the layer's own `Rc`, so two
    /// clones of one handle answer `true` and two separate layers never do.
    fn belongs_to(&self, layer: &WetLayer) -> bool {
        Rc::ptr_eq(&self.layer.0, &layer.0)
    }
}

impl Widget for WetLayerNode {
    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::ZERO.into()
    }

    /// Required by [`WetLayer::request_repaint`]: it reaches this node through
    /// `EventContext::with_widget_mut`, which is the only door that resolves a
    /// [`WidgetId`] against the **calling** tree and type-checks what it finds.
    /// Without this override the arena hands back nothing and a wet surface
    /// silently stops repainting.
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn paint(&self, _bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let region = self.bridge.visible_region();
        let item_ctx =
            SceneItemPaintContext::new(self.bridge.view_transform(), Some(region), ctx.theme)
                .with_text_scale(ctx.text_scale)
                .with_window_active(ctx.window_active)
                .with_enabled(ctx.effective_enabled);
        self.layer.paint_with(canvas, &item_ctx);
    }

    fn accessibility(&self, builder: &mut teksilo_core::accessibility::AccessNodeBuilder) {
        // A gesture in flight is not an object. The finished one becomes a
        // scene item, with whatever name the app gives it; announcing a
        // half-drawn stroke would be noise a screen reader cannot act on.
        builder.set_hidden();
    }
}
