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
//!
//! The per-item rendering those two share — z-sorting, item-coordinate GPU
//! caching, `IGNORES_TRANSFORMATIONS` pinning, per-item opacity composition and
//! glyph-epoch eviction recovery — lives in
//! [`ScenePaintBridge::paint_items`](super::paint_node::ScenePaintBridge::paint_items),
//! because the [`Interleaved`](crate::SceneLayer::Interleaved) band paints from
//! nodes of its own and must render an item identically. `paint_band` here is
//! the band's candidate query and nothing more.

use super::*;

impl SceneView {
    /// Paint one lightweight band into `canvas`, under the active view
    /// transform. Queries the visible items, keeps only those in `band`, and
    /// hands them to
    /// [`ScenePaintBridge::paint_items`](super::paint_node::ScenePaintBridge::paint_items)
    /// — the one copy of the per-item rendering, shared with the `Interleaved`
    /// band's own paint nodes.
    ///
    /// Called with [`Under`](crate::SceneLayer::Under) from
    /// [`paint_impl`](Self::paint_impl) (the backdrop, before the children) and
    /// with [`Over`](crate::SceneLayer::Over) from
    /// [`post_paint_impl`](Self::post_paint_impl) (the foreground, after them).
    /// [`Interleaved`](crate::SceneLayer::Interleaved) is deliberately not
    /// reachable from here: those items paint from their own nodes, in the
    /// middle of the child walk, which is the entire point of the band.
    fn paint_band(
        &self,
        canvas: &mut teksilo_canvas::Canvas,
        bounds: Rect,
        band: crate::scene::SceneLayer,
        ctx: &PaintContext,
    ) {
        debug_assert_ne!(
            band,
            crate::scene::SceneLayer::Interleaved,
            "the Interleaved band paints from SceneBandProxy nodes, not from a band pass"
        );
        let region = self.visible_scene_region(bounds);
        let ids = {
            let scene = self.model.0.borrow();
            let mut ids = scene.items_in_rect(region);
            // Paint order: bottom-most first, so a later element paints on top.
            // The comparator is `PaintKey`, the *same* value every hit test in
            // the crate compares — which is what makes "what the user sees on
            // top" and "what the pointer picks" one rule instead of two. Within
            // a band rank is constant, so this reduces to ascending z with
            // equal-z resolved in insertion order.
            scene.sort_by_paint_key(&mut ids);
            ids.retain(|id| scene.layer(*id) == Some(band));
            ids
        };
        self.paint_bridge().paint_items(&ids, canvas, region, ctx);
    }

    pub(super) fn paint_impl(
        &self,
        bounds: Rect,
        canvas: &mut teksilo_canvas::Canvas,
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
        // Republish for the child paint nodes — a `SceneBandProxy` or a
        // `WetLayerNode` knows its own rect, not the viewport's. See
        // `view::paint_node`.
        self.published_visible_region.set(region);

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
            || self.transform_wants_post_paint()
    }

    pub(super) fn post_paint_impl(
        &self,
        bounds: Rect,
        canvas: &mut teksilo_canvas::Canvas,
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
                let fill = teksilo_tokens::Color::new(0.40, 0.55, 0.85, 0.18);
                let stroke = teksilo_tokens::Color::new(0.40, 0.55, 0.85, 0.85);
                canvas.fill_rect(scene_rect, fill);
                canvas.stroke_rect(scene_rect, stroke, teksilo_canvas::StrokeStyle::solid(1.0));
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

        // The selection transform frame and its handles, over the content and
        // over the magnet feedback (they are what the pointer grabs first), and
        // still under the debug overlay. A paint pass, not scene items — see
        // `view::transform` for why that distinction is load-bearing.
        self.paint_transform_chrome(canvas, ctx);

        // Visual-debug overlays, on top of everything.
        if self.debug_overlay.is_active() {
            self.paint_debug_overlay(bounds, canvas);
        }
    }

    /// Paint enabled debug overlays on top of the scene rendering.
    /// All paint commands are in scene coords — they ride the
    /// same view-transform scope as items, so the overlays follow
    /// the user's pan/zoom naturally.
    fn paint_debug_overlay(&self, bounds: Rect, canvas: &mut teksilo_canvas::Canvas) {
        let cfg = self.debug_overlay;
        let region = self.visible_scene_region(bounds);
        let stroke_w = 1.0;
        // Distinct color per overlay so multiple flags compose
        // visually without confusion.
        let item_color = teksilo_tokens::Color::new(0.20, 0.75, 0.35, 0.85);
        let content_color = teksilo_tokens::Color::new(0.30, 0.45, 0.95, 0.85);
        let viewport_color = teksilo_tokens::Color::new(0.95, 0.30, 0.30, 0.85);
        let selection_color = teksilo_tokens::Color::new(1.00, 0.60, 0.20, 0.95);

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
                        teksilo_canvas::StrokeStyle::solid(stroke_w),
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
                teksilo_canvas::StrokeStyle::solid(stroke_w),
            );
        }
        if cfg.viewport {
            canvas.stroke_rect(
                region,
                viewport_color,
                teksilo_canvas::StrokeStyle::solid(stroke_w),
            );
        }
        if cfg.selection_bounds {
            for id in self.selection.selected() {
                if let Some(rect) = visible_bounds(id) {
                    canvas.stroke_rect(
                        rect,
                        selection_color,
                        teksilo_canvas::StrokeStyle::solid(stroke_w * 2.0),
                    );
                }
            }
        }
    }

    // -- A11y-walker helpers used by `accessibility` -----------------------
}
