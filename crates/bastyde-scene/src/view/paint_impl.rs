// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Paint implementation for [`SceneView`].
//!
//! Implements the three painting entry points called by the framework walker:
//! `paint_impl` (the `Under`-band lightweight items and the app background
//! closure, rendered before heavyweight children), `wants_post_paint_impl`
//! (guard that avoids an empty foreground pass), and `post_paint_impl` (the
//! `Over`-band items, selection marquee, app foreground hook, magnetism
//! feedback, and debug overlays, all rendered after heavyweight children).
//! The shared `paint_band` helper handles z-sorting, item-coordinate GPU
//! caching, `IGNORES_TRANSFORMATIONS` pinning, per-item opacity composition,
//! and glyph-epoch eviction recovery.

use super::*;

impl SceneView {
    /// Paint one lightweight band into `canvas`, under the active view
    /// transform. Queries the visible items, z-sorts them, keeps only those
    /// in `band`, and paints each. Heavyweight ids are skipped (they paint
    /// via the arena walker). Shared by [`paint`](Self::paint) (the `Under`
    /// backdrop, before the children) and [`post_paint`](Self::post_paint)
    /// (the `Over` foreground, after the children). Within a band, `z`
    /// orders items among themselves.
    fn paint_band(
        &self,
        canvas: &mut bastyde_canvas::Canvas,
        bounds: Rect,
        band: crate::scene::SceneLayer,
        ctx: &PaintContext,
    ) {
        // Glyph-epoch gate: cached item frames bake glyph atlas UVs, and
        // this cache lives outside the widget arena, so the framework's
        // eviction recovery (`invalidate_all_paints`) cannot reach it.
        // Instead, every paint pass compares the backend's eviction
        // epoch and drops all entries when it moved — the items repaint
        // below with fresh UVs in the same pass.
        let glyph_epoch = canvas
            .text_backend()
            .map(|tb| tb.borrow().glyph_epoch())
            .unwrap_or(0);
        self.item_cache.borrow_mut().sync_glyph_epoch(glyph_epoch);

        // Ambient text raster scale, set by the paint walker for this
        // SceneView's content-transform scope (it already includes the
        // view's zoom). Items whose own pushed transform carries an
        // additional scale refine it below so their text rasterizes at
        // the full effective density.
        let ambient_raster_scale = canvas
            .text_backend()
            .map(|tb| tb.borrow().raster_scale())
            .unwrap_or(1.0);

        let region = self.visible_scene_region(bounds);
        let view_transform = self.view_transform();
        // Built once per band; `enabled` is refreshed per item inside the loop
        // (it is the only field that varies per item). `theme` and
        // `window_active` come straight from the widget paint pass, so
        // lightweight items resolve theme roles and desaturate on window blur
        // exactly like widgets.
        let mut item_ctx =
            crate::item::SceneItemPaintContext::new(view_transform, Some(region), ctx.theme)
                .with_text_scale(ctx.text_scale)
                .with_window_active(ctx.window_active);
        let drag_target = self.drag_target.get();
        let mut visible_ids = self.scene().items_in_rect(region);
        // Z-order within the band: higher z paints last (on top); equal-z
        // preserves insertion order (stable sort). Heavyweight ids stay in the
        // list but are skipped below — they paint via the arena walker.
        self.scene().sort_by_z(&mut visible_ids);
        for id in visible_ids {
            let scene = self.model.0.borrow();
            if scene.item(id).is_none() {
                continue;
            }
            // Only this band; items default to Under.
            if scene.layer(id).unwrap_or(crate::scene::SceneLayer::Under) != band {
                continue;
            }
            // Skip items whose chain is invisible or which carry the
            // HAS_NO_CONTENTS flag (logical-only).
            if !self.scene().is_effectively_visible(id) {
                continue;
            }
            let flags = self.scene().flags(id).unwrap_or_default();
            if flags.contains(crate::flags::ItemFlags::HAS_NO_CONTENTS) {
                continue;
            }
            // Per-item enabled state drives `ColorProp` disabled-role resolution.
            // AND-combine the item's own `IS_ENABLED` flag with the widget-tree's
            // ancestor-disabled cascade (`ctx.effective_enabled`), so a SceneView
            // inside a disabled ancestor dims its lightweight items' role colours
            // exactly like every other widget in that subtree.
            item_ctx.enabled =
                ctx.effective_enabled && flags.contains(crate::flags::ItemFlags::IS_ENABLED);
            // Items that are the drag target or a declared descendant paint
            // with a visual delta in scene coords — a child follows its
            // dragged parent until the rebuild commits the new local_pos.
            let drag_delta = drag_target
                .filter(|t| t.item_id == id || self.scene().is_descendant_of(id, t.item_id))
                .map(|t| {
                    bastyde_canvas::Transform2D::translate(
                        t.current_scene.x - t.anchor_scene.x,
                        t.current_scene.y - t.anchor_scene.y,
                    )
                });

            // Compose `local→scene`, optionally with a scene-coord drag
            // offset baked in. Push beneath the view transform so the item's
            // `paint` works in local coords. `save` / `restore` isolate
            // neighbouring items' transforms.
            let mut local_to_scene = self.scene().scene_transform(id);
            if let Some(t) = drag_delta {
                local_to_scene = local_to_scene.then(&t);
            }
            canvas.save();
            // IGNORES_TRANSFORMATIONS items pin at their parent-relative
            // position but render at a fixed pixel size (Qt's
            // `ItemIgnoresTransformations`). Project the anchor through the
            // parent chain + view transform, then push a transform that —
            // composed with the outer view transform on the canvas —
            // collapses to a pure `Translate(screen_anchor)`.
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
            // Refine the ambient raster scale by the item's own pushed
            // transform scale: a scaled item's text needs denser bitmaps
            // (and a pixel-pinned IGNORES_TRANSFORMATIONS item, whose
            // pushed transform carries `1/view_scale`, falls back toward
            // 1.0 — it never zooms on screen). `quantize_raster_scale`
            // is idempotent, so scale-1 transforms inherit the ambient
            // value bit-identically and the backend is left untouched.
            let item_raster_scale = bastyde_canvas::quantize_raster_scale(
                ambient_raster_scale * pushed_transform.geometric_scale(),
            );
            let raster_scale_changed = item_raster_scale != ambient_raster_scale;
            if raster_scale_changed && let Some(tb) = canvas.text_backend() {
                tb.borrow_mut().set_raster_scale(item_raster_scale);
            }
            // Effective opacity composes through the parent chain. Pushed via
            // `set_opacity` / `restore_opacity` so the scope is balanced.
            let alpha = self.scene().effective_opacity(id);
            let opacity_pushed = alpha < 0.999;
            if opacity_pushed {
                canvas.set_opacity(alpha);
            }
            if let Some(item) = scene.item(id) {
                // Item-coordinate cache: replay a cached local-coord
                // RenderFrame instead of re-running paint when the item opted
                // into `CacheMode::ItemCoordinate`; record on a miss. The
                // cache keys each entry by the raster scale it was recorded
                // at, so a zoom that crossed a raster bucket re-records the
                // frame against fresh-density bitmaps.
                match item.cache_mode() {
                    crate::cache::CacheMode::ItemCoordinate => {
                        let cached = self.item_cache.borrow().get(id, item_raster_scale).cloned();
                        if let Some(frame) = cached {
                            canvas.draw_render_frame(&frame, Point::ZERO);
                        } else {
                            let mut sub = match canvas.text_backend() {
                                Some(tb) => bastyde_canvas::Canvas::with_text_backend(tb.clone()),
                                None => bastyde_canvas::Canvas::new(),
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

    pub(super) fn paint_impl(
        &self,
        bounds: Rect,
        canvas: &mut bastyde_canvas::Canvas,
        ctx: &PaintContext,
    ) {
        // Sync `bounds_origin_signal` with the bounds the framework
        // assigned. `place_children` is the canonical site for this,
        // but it only runs when the SceneView has heavyweight widget
        // children — a SceneView with only lightweight items would
        // never see a place_children call and its bounds_origin
        // would stay at its default. That breaks nested-SceneView
        // placement (the inner view draws at outer-scene origin
        // instead of at its own scene_rect). Updating from `paint`
        // costs a one-frame lag on first display (the Signal change
        // dirties view_transform_signal which dirties paint for the
        // next frame). For static nested SceneViews this is
        // unnoticeable; for moving ones, every frame the bounds
        // change, the signal updates, and the next frame catches up.
        let new_origin = Vec2::new(bounds.x, bounds.y);
        if self.bounds_origin_signal.get() != new_origin {
            self.bounds_origin_signal.set(new_origin);
        }

        // The SceneView's `set_content_transform` scope wraps both this paint
        // call and the children walk, so any `canvas.fill_*` /
        // `canvas.stroke_*` / `canvas.draw_*` call we make here lands
        // through the same view-transform projection as the heavyweight
        // children. We pass scene-coord rects directly — the renderer
        // composes pan / zoom / rotation / bounds-origin on top.
        let region = self.visible_scene_region(bounds);

        // App-supplied background closure: paints under all items in
        // scene coords, with the visible scene region passed so the
        // closure can skip off-screen geometry.
        if let Some(bg) = &self.background_paint {
            bg(canvas, ctx, region);
        }

        // Under band: lightweight items below the heavyweight children
        // (background furniture — connector lines, tiled grids, decorations).
        // The render walker invokes the parent's paint first, then descends
        // into children, so these render under the cards. The Over band and
        // the marquee / foreground / debug overlays paint in `post_paint`
        // (after the children) so they sit on top.
        self.paint_band(canvas, bounds, crate::scene::SceneLayer::Under, ctx);
    }

    pub(super) fn wants_post_paint_impl(&self) -> bool {
        // SceneView has a foreground pass (post_paint) only when something
        // must render over the heavyweight children: a selection marquee, an
        // app-supplied foreground closure, debug overlays, or any lightweight
        // item raised to the Over band.
        self.marquee.get().is_some()
            || self.foreground_paint.is_some()
            || self.debug_overlay.is_active()
            || self.scene().has_over_layer_items()
            || self.magnet_wants_post_paint()
    }

    pub(super) fn post_paint_impl(
        &self,
        bounds: Rect,
        canvas: &mut bastyde_canvas::Canvas,
        ctx: &PaintContext,
    ) {
        // Foreground pass — runs after the heavyweight children, inside the
        // same view-transform / clip scope as `paint`. Everything here sits
        // on top of the cards: the Over-band lightweight items, then the
        // selection marquee, then the app foreground hook, then debug overlays.
        let view_transform = self.view_transform();

        // Over band: lightweight items explicitly raised above the cards
        // (highlighted connectors, selection halos, annotations).
        self.paint_band(canvas, bounds, crate::scene::SceneLayer::Over, ctx);

        // Marquee overlay — semi-transparent fill plus a single-pixel
        // stroke. The marquee state is in screen coords (set by the on_drag
        // closure); the view-transform scope is on the canvas, so we project
        // the screen-rect back to scene coords and paint there.
        if let Some(state) = self.marquee.get() {
            let screen_rect = state.rect();
            if let Some(inv) = view_transform.inverse() {
                let scene_rect = inv.apply_rect(screen_rect);
                let fill = bastyde_tokens::Color::new(0.40, 0.55, 0.85, 0.18);
                let stroke = bastyde_tokens::Color::new(0.40, 0.55, 0.85, 0.85);
                canvas.fill_rect(scene_rect, fill);
                canvas.stroke_rect(scene_rect, stroke, bastyde_canvas::StrokeStyle::solid(1.0));
            }
        }

        // App-supplied foreground closure: paints over all items (and the
        // marquee), under the debug overlay.
        if let Some(fg) = &self.foreground_paint {
            let region = self.visible_scene_region(bounds);
            fg(canvas, ctx, region);
        }

        // Magnetism feedback (markers + connector / ghost wire), over the
        // content and the app foreground, under the debug overlay. Same
        // scene-coord scope.
        self.paint_magnet_feedback(bounds, canvas, ctx);

        // Visual-debug overlays, on top of everything.
        if self.debug_overlay.is_active() {
            self.paint_debug_overlay(bounds, canvas);
        }
    }

    /// Paint enabled debug overlays on top of the scene rendering.
    /// All paint commands are in scene coords — they ride the
    /// same view-transform scope as items, so the overlays follow
    /// the user's pan/zoom naturally.
    fn paint_debug_overlay(&self, bounds: Rect, canvas: &mut bastyde_canvas::Canvas) {
        let cfg = self.debug_overlay;
        let region = self.visible_scene_region(bounds);
        let stroke_w = 1.0;
        // Distinct color per overlay so multiple flags compose
        // visually without confusion.
        let item_color = bastyde_tokens::Color::new(0.20, 0.75, 0.35, 0.85);
        let content_color = bastyde_tokens::Color::new(0.30, 0.45, 0.95, 0.85);
        let viewport_color = bastyde_tokens::Color::new(0.95, 0.30, 0.30, 0.85);
        let selection_color = bastyde_tokens::Color::new(1.00, 0.60, 0.20, 0.95);

        // The canvas rides the view-transform scope, so paint commands
        // are in scene coords. For IGNORES_TRANSFORMATIONS items, the
        // visible area on screen is `local_bounds` rooted at the
        // screen-projected `scene_anchor` (NOT at the zoom-scaled
        // scene_rect). To stroke that visible area correctly through
        // a view-transform-scoped canvas we inverse-project the
        // screen-space rect back to scene coords — same trick the
        // marquee overlay uses. Falls through to scene_rect when the
        // view transform is degenerate.
        let view_transform = self.view_transform();
        let visible_bounds = |id: crate::item::ItemId| -> Option<Rect> {
            let scene_rect = self.scene().scene_rect(id)?;
            let flags = self.scene().flags(id).unwrap_or_default();
            if !flags.contains(crate::flags::ItemFlags::IGNORES_TRANSFORMATIONS) {
                return Some(scene_rect);
            }
            let local_bounds = self.scene().local_bounds(id)?;
            let scene_xform = self.scene().scene_transform(id);
            let scene_anchor = scene_xform.apply_point(Point::ZERO);
            let screen_anchor = view_transform.apply_point(scene_anchor);
            let screen_rect = Rect::new(
                screen_anchor.x + local_bounds.x,
                screen_anchor.y + local_bounds.y,
                local_bounds.width,
                local_bounds.height,
            );
            view_transform
                .inverse()
                .map(|inv| inv.apply_rect(screen_rect))
                .or(Some(scene_rect))
        };

        if cfg.item_bounds {
            for id in self.scene().ids() {
                if let Some(rect) = visible_bounds(id) {
                    canvas.stroke_rect(
                        rect,
                        item_color,
                        bastyde_canvas::StrokeStyle::solid(stroke_w),
                    );
                }
            }
        }
        if cfg.content_bounds
            && let Some(content) = self.scene_content_bounds()
        {
            canvas.stroke_rect(
                content,
                content_color,
                bastyde_canvas::StrokeStyle::solid(stroke_w),
            );
        }
        if cfg.viewport {
            canvas.stroke_rect(
                region,
                viewport_color,
                bastyde_canvas::StrokeStyle::solid(stroke_w),
            );
        }
        if cfg.selection_bounds {
            for id in self.selection.selected() {
                if let Some(rect) = visible_bounds(id) {
                    canvas.stroke_rect(
                        rect,
                        selection_color,
                        bastyde_canvas::StrokeStyle::solid(stroke_w * 2.0),
                    );
                }
            }
        }
    }

    // -- A11y-walker helpers used by `accessibility` -----------------------
}
