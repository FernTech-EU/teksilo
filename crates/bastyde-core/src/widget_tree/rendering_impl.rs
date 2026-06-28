// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use super::*;

use bastyde_canvas::quantize_raster_scale;

/// Accessibility preferences passed through the paint recursion.
struct A11yPaintPrefs {
    high_contrast: bool,
    reduced_motion: bool,
    large_text: bool,
}

impl WidgetTree {
    /// Paint all active widgets and produce a RenderFrame.
    /// Uses per-widget paint caching: only widgets with `needs_paint` are
    /// re-painted; clean widgets reuse their cached paint output.
    /// Also caches the full assembled frame — if no widget needs painting,
    /// the previous frame is returned immediately.
    pub fn render(&mut self) -> std::rc::Rc<RenderFrame> {
        let mut noop = crate::window::NoopWindowOps;
        self.render_with_ops(&mut noop)
    }

    /// Render a frame, threading the app's
    /// [`WindowOps`](crate::window::WindowOps) sink through any
    /// state-change-triggered handlers (data-driven rebuild, binding
    /// flush). Called by `bastyde-app` during its paint pipeline.
    pub fn render_with_ops(
        &mut self,
        ops: &mut dyn crate::window::WindowOps,
    ) -> std::rc::Rc<RenderFrame> {
        self.process_state_changes(&mut *ops);

        // Always tick the animated-quad registry — even on the cache-hit
        // early-out we still need fresh phase in the frame's
        // anim_params, because every looping slot advances. Widgets
        // whose paint() wasn't re-run (all of them on cache hit) keep
        // their last DrawCommand::AnimatedQuad in the cached frame;
        // the renderer reads the live params from `frame.anim_params`
        // at the slot index stored in the draw command.
        //
        // The registry's internal `scratch` buffer owns the params; we
        // only copy when actually writing them into the frame (below).
        // Taking a borrow here lets us skip the copy entirely in the
        // non-cache-hit branch where we allocate a fresh frame anyway.
        let now = std::time::Instant::now();
        let has_animations = self.animated_quads.has_running();
        if has_animations {
            self.animated_quads
                .tick(now, &self.arena, self.paint_epoch, &self.theme);
        }

        // Cache-hit short-circuit: nothing in the tree was marked
        // needs_paint, so the pixels are identical to the previous
        // frame apart from the animated-quad uniforms. We deliberately
        // do NOT bump `paint_epoch` here — if we did, every widget's
        // `last_painted_epoch` would silently age out and the animation
        // scheduler would treat them as "off-screen" on the next tick.
        // Holding the epoch steady preserves the visibility gate
        // through arbitrarily many idle cache-hit frames. The fresh
        // `anim_params` we just computed are attached so shader-driven
        // animations keep advancing even when paint() doesn't run.
        //
        // `Rc::make_mut` short-circuits to a mutable borrow when the
        // tree is the sole owner of the cached frame — which is the
        // common case, since the app-side caller typically drops the
        // previous frame before calling `render()` again. If the
        // caller holds a second Rc clone (e.g. two back-to-back
        // renders without letting the first drop) we fall back to a
        // single deep clone for that frame.
        if !self.arena.any_needs_paint() && self.cached_frame.is_some() {
            // Cache-hit path. paint_epoch is frozen, so visible
            // subscribers' `last_painted_epoch == paint_epoch` still
            // holds. Re-arm the frame-tick chain BEFORE refreshing the
            // cached frame so we don't tangle borrows.
            self.arm_frame_tick_for_visible_subscribers();

            if let Some(cached) = self.cached_frame.as_mut() {
                let frame = std::rc::Rc::make_mut(cached);
                if has_animations {
                    let src = self.animated_quads.scratch_slice();
                    frame.anim_params.clear();
                    frame.anim_params.extend_from_slice(src);
                }
                // Refresh the text backend's glyph timestamps for every
                // layout baked into the reused frame — same contract as
                // the per-widget cached_paint path below. Without this,
                // a window that idles on the full-frame cache while
                // text work elsewhere (another window, or measure-only
                // layout passes) advances the atlas generation would
                // have its still-visible glyphs evicted and their atlas
                // slots reused, garbling the cached quads.
                if !frame.layout_keys.is_empty()
                    && let Some(tb) = &self.text_backend
                {
                    let mut tb = tb.borrow_mut();
                    for key in &frame.layout_keys {
                        tb.touch_layout(*key);
                    }
                    #[cfg(debug_assertions)]
                    debug_validate_layout_keys(&*tb, &frame.layout_keys, None);
                }
                return std::rc::Rc::clone(cached);
            }
        }

        self.paint_epoch = self.paint_epoch.saturating_add(1);
        let paint_epoch = self.paint_epoch;

        let mut frame = RenderFrame::new();
        // `effective_theme` carries the user/OS text-scale multiplier baked into
        // its typography, so painted glyphs match the scaled layout sizes.
        let base_theme = self.effective_theme.clone();
        let text_backend = self.text_backend.clone();
        let a11y_prefs = A11yPaintPrefs {
            high_contrast: self.prefers_high_contrast,
            reduced_motion: self.prefers_reduced_motion,
            large_text: self.text_scale_factor > 1.0,
        };

        let overlay_skip: std::collections::HashSet<WidgetId> = self
            .overlay_manager
            .active_content_ids()
            .into_iter()
            .collect();

        for root_id in self.arena.roots() {
            // Don't descend into overlay content via its anchor parent — it
            // is painted via the dedicated overlay loop below. Without this,
            // the overlay content paints twice per frame.
            if overlay_skip.contains(&root_id) {
                continue;
            }
            paint_widget_cached(
                &mut self.arena,
                root_id,
                &mut frame,
                &base_theme,
                &text_backend,
                None,
                &a11y_prefs,
                paint_epoch,
                &overlay_skip,
                self.layout_direction,
                // Root starts enabled; the walker ANDs in each node's
                // own `enabled_state` as it descends.
                true,
                // Root is at screen scale; transform scopes below
                // multiply in their own scale.
                1.0,
            );
        }

        for content_id in self.overlay_manager.active_content_ids() {
            paint_widget_cached(
                &mut self.arena,
                content_id,
                &mut frame,
                &base_theme,
                &text_backend,
                None,
                &a11y_prefs,
                paint_epoch,
                &overlay_skip,
                self.layout_direction,
                // Overlays detach from their anchor's enabled-state.
                // A tooltip / popover stays enabled even if its
                // anchor was disabled — overlays receive their own
                // explicit enabled-state if they need one.
                true,
                // Overlays render at screen scale regardless of their
                // anchor's transform scopes.
                1.0,
            );
        }

        // Clear `needs_paint` on every active node. Mutation during
        // iter — fill the reusable scratch first (zero-alloc once
        // warm), then drive the loop from the snapshot.
        self.arena.fill_active_ids(&mut self.active_ids_scratch);
        let ids = std::mem::take(&mut self.active_ids_scratch);
        for &id in &ids {
            if let Some(node) = self.arena.get_mut(id) {
                node.dirty.needs_paint = false;
            }
        }
        self.active_ids_scratch = ids;

        if has_animations {
            frame
                .anim_params
                .extend_from_slice(self.animated_quads.scratch_slice());
        }
        frame.debug_validate_stacks();
        let rc = std::rc::Rc::new(frame);
        self.cached_frame = Some(std::rc::Rc::clone(&rc));
        // Full-render path: subscribers whose owners were just painted
        // have `last_painted_epoch == paint_epoch`. Re-arm the
        // frame-tick chain so per-frame effects keep ticking. When all
        // subscribers are off-screen this returns false and the chain
        // dies — exactly the bug fix that motivated this scheduler.
        self.arm_frame_tick_for_visible_subscribers();
        rc
    }

    /// Walk the frame-tick subscriber set and arm `frame_tick_requested`
    /// if any subscriber's owner is currently visible. Called from
    /// `render_with_ops` (both cache-hit and full-render paths).
    pub(crate) fn arm_frame_tick_for_visible_subscribers(&self) {
        if self
            .frame_tick_scheduler
            .should_arm_frame_tick(&self.arena, self.paint_epoch)
        {
            self.frame_tick_requested.set(true);
        }
    }
}

/// Recursive paint pass with per-widget caching.
/// Only re-runs `paint()` for widgets with `needs_paint` set; clean widgets
/// reuse their `cached_paint` output. The tree walk still runs for clip/child
/// ordering, but skips the expensive `paint()` call for clean widgets.
///
/// `parent_effective_enabled` is the AND of every ancestor's `enabled_state`
/// resolved value (start `true` at root). The walker ANDs this with the
/// current node's `enabled_state` to produce `this_effective_enabled`, which
/// it both injects into the node's `PaintContext` and forwards as the parent
/// value to children. This is the single mechanism by which leaf widgets see
/// the arena's enabled-state at paint time without needing to walk ancestors
/// themselves (`PaintContext` carries no `WidgetId` or arena reference).
///
/// `accumulated_raster_scale` is the quantized product of every ancestor
/// transform scope's scale (start `1.0` at root). The walker multiplies in
/// this node's own transform scale, sets the result as the text backend's
/// ambient raster scale around the node's paint (so text drawn here
/// rasterizes densely enough for the GPU transform that will stretch it),
/// stamps it on the node's paint cache, and forwards it to children.
#[allow(clippy::too_many_arguments)]
fn paint_widget_cached(
    arena: &mut WidgetArena,
    id: WidgetId,
    frame: &mut RenderFrame,
    base_theme: &crate::styles::Theme,
    text_backend: &Option<Rc<RefCell<dyn bastyde_canvas::TextBackend>>>,
    clip_bounds: Option<Rect>,
    a11y_prefs: &A11yPaintPrefs,
    paint_epoch: u64,
    overlay_skip: &std::collections::HashSet<WidgetId>,
    layout_direction: crate::environment::LayoutDirection,
    parent_effective_enabled: bool,
    accumulated_raster_scale: f32,
) {
    if !arena.is_active(id) {
        return;
    }

    // Compute this node's effective enabled-state once: AND the
    // ancestor-derived value with our own `enabled_state` if set.
    // Used both for our own paint context and for the recursion into
    // children below.
    let this_effective_enabled = parent_effective_enabled
        && arena
            .get(id)
            .and_then(|n| n.enabled_state.as_ref())
            .is_none_or(|p| p.get());

    let node = arena.get(id).expect("node id is active (guarded above)");
    let bounds = node.bounds;
    if let Some(clip) = clip_bounds {
        let x0 = bounds.x.max(clip.x);
        let y0 = bounds.y.max(clip.y);
        let x1 = bounds.right().min(clip.right());
        let y1 = bounds.bottom().min(clip.bottom());
        if x1 <= x0 || y1 <= y0 {
            // Clipped to nothing — widget is offscreen. Skip its paint
            // AND skip stamping `last_painted_epoch`, so the animation
            // scheduler will notice it is no longer visible and pause
            // its looping animations.
            return;
        }
    }

    // Mark this widget as "painted in epoch N" regardless of whether
    // we hit the cache-path or ran `paint()` — both outcomes mean the
    // widget's bounds landed inside the viewport this frame, which is
    // all the animation scheduler cares about.
    if let Some(node_mut) = arena.get_mut(id) {
        node_mut.last_painted_epoch = paint_epoch;
    }

    // Read the optional opacity scope before borrowing the node again
    // for the paint/cache path. The scope wraps both this widget's own
    // paint and its children's paint, composing with any ancestor
    // opacity via the canvas's stacked-opacity model.
    let node = arena.get(id).expect("node id is active (guarded above)");
    let opacity = node.opacity_prop.as_ref().map(|p| p.get().clamp(0.0, 1.0));
    if let Some(o) = opacity
        && o < 1.0 / 512.0
    {
        // Sub-perceptual: skip the subtree entirely. Saves a draw
        // pass when a `Fade` is fully transparent (e.g. just-dismissed
        // tooltip waiting for cleanup) and prevents 0.0 from emitting
        // a spurious blend pass on the GPU. We do this before any blur
        // scope emit so the Begin/End pair stays balanced.
        return;
    }

    // Optional blur scope is the OUTERMOST per-node scope: it captures
    // the entire rendered subtree (including any opacity/transform scopes
    // we're about to push) into an intermediate texture, blurs it via
    // the renderer's dual-Kawase chain, and composites the blurred
    // result back at the widget's bounds. Sub-perceptual radii skip the
    // pair entirely so animated 0→target_radius patterns have zero cost
    // when fully off.
    let node = arena.get(id).expect("node id is active (guarded above)");
    let blur_radius = node
        .blur_prop
        .as_ref()
        .map(|p| p.get())
        .filter(|r| *r >= 0.5);
    let blur_bounds = arena.bounds(id);
    if let Some(r) = blur_radius {
        frame
            .draw_order
            .push(bastyde_canvas::DrawCommand::BeginBlurredSubtree {
                bounds: blur_bounds,
                radius: r,
            });
    }

    if let Some(o) = opacity {
        frame
            .draw_order
            .push(bastyde_canvas::DrawCommand::SetOpacity(o));
    }

    // Optional transform scope — wraps both this widget's own paint
    // and its children's paint. The renderer composes the pushed
    // transform onto its stack so widget-internal canvas transforms
    // (canvas.translate / scale / rotate) compose with this wrapper
    // transform instead of clobbering it. Skip the push entirely when
    // the transform is identity — saves a flush on every wrapper that
    // happens to be at its rest pose.
    let node = arena.get(id).expect("node id is active (guarded above)");
    let transform = node.transform_prop.as_ref().map(|p| p.get());
    let push_transform = transform.filter(|t| *t != bastyde_canvas::Transform2D::IDENTITY);
    let clips = node.clips_children;
    let content_transform = node.content_transform;
    let bounds = node.bounds;

    // Accumulated raster scale for this node and its subtree: multiply
    // the ancestors' scale by this node's own transform scale (a
    // SceneView zoom or a `Scale` wrapper) and re-quantize. Text painted
    // inside the scope rasterizes at this density so the GPU transform
    // lands a ~1:1 texel-to-pixel mapping instead of stretching a 1×
    // bitmap. Pure translations/rotations have `geometric_scale() == 1`
    // and inherit the parent value bit-identically.
    let this_raster_scale = match push_transform {
        Some(t) => quantize_raster_scale(accumulated_raster_scale * t.geometric_scale()),
        None => accumulated_raster_scale,
    };
    // The backend currently holds the parent's ambient scale (root
    // callers start at 1.0; every recursion level restores on exit), so
    // it only needs touching when this node's scale differs.
    let raster_scale_changed = this_raster_scale != accumulated_raster_scale;
    if raster_scale_changed && let Some(tb) = text_backend {
        tb.borrow_mut().set_raster_scale(this_raster_scale);
    }
    // A *content* transform (a SceneView's pan/zoom) leaves the node's bounds a
    // fixed parent-space viewport and only moves the content. Emit its clip
    // BEFORE the transform so the renderer scissors to that fixed viewport
    // (transformed by ancestors only — correct for nested scenes too) instead
    // of the pan/zoom-shifted rect, and so the node's own paint (background
    // grid / lightweight items) is clipped to the viewport as well. A *self*
    // transform (Scale/Rotate) keeps its clip INSIDE the transform — there the
    // clip is meant to be the scaled visual region.
    let clip_outside_transform = clips && content_transform;
    if clip_outside_transform {
        frame
            .draw_order
            .push(bastyde_canvas::DrawCommand::SetClip(bounds));
    }
    if let Some(t) = push_transform {
        frame
            .draw_order
            .push(bastyde_canvas::DrawCommand::PushTransform(t));
    }

    let node = arena.get(id).expect("node id is active (guarded above)");
    // A clean widget's cached frames bake glyph quads at the raster
    // scale current when they were recorded; when the ambient scale
    // moved (a scene zoom crossed a quantization bucket), those quads
    // sample wrong-density bitmaps — treat the node as needing paint.
    let needs_paint = node.dirty.needs_paint || node.paint_raster_scale != this_raster_scale;

    if needs_paint || node.cached_paint.is_none() {
        let resolved_theme = arena.resolve_theme(id, base_theme);
        let ctx = PaintContext {
            theme: &resolved_theme,
            scale_factor: this_raster_scale,
            layout_direction,
            effective_enabled: this_effective_enabled,
            prefers_high_contrast: a11y_prefs.high_contrast,
            prefers_reduced_motion: a11y_prefs.reduced_motion,
            prefers_large_text: a11y_prefs.large_text,
        };

        let bounds = arena.bounds(id);
        let node = arena.get(id).expect("node id is active (guarded above)");

        let mut canvas = match text_backend {
            Some(tb) => Canvas::with_text_backend(tb.clone()),
            None => Canvas::new(),
        };
        node.widget.paint(bounds, &mut canvas, &ctx);
        let widget_frame = canvas.into_render_frame();

        frame.merge(&widget_frame);
        if let Some(node) = arena.get_mut(id) {
            node.cached_paint = Some(widget_frame);
            node.paint_raster_scale = this_raster_scale;
        }
    } else {
        let node = arena.get(id).expect("node id is active (guarded above)");
        if let Some(cached) = &node.cached_paint {
            // Refresh the text backend's glyph timestamps for every
            // layout baked into this cached paint. Without this,
            // widgets that stay clean for ~180 frames (e.g. static
            // labels next to an animation) can have their atlas slots
            // evicted and reused, and the cached UVs then sample the
            // wrong glyph. `TextBackend::touch_layout` is a no-op for
            // backends without a glyph cache (the mock).
            if !cached.layout_keys.is_empty()
                && let Some(tb) = text_backend
            {
                let mut tb = tb.borrow_mut();
                for key in &cached.layout_keys {
                    tb.touch_layout(*key);
                }
                #[cfg(debug_assertions)]
                debug_validate_layout_keys(&*tb, &cached.layout_keys, Some(id));
            }
            frame.merge(cached);
        }
    }

    let node = arena.get(id).expect("node id is active (guarded above)");
    let children: Vec<WidgetId> = node.children.clone();
    let next_clip = if clips {
        Some(match clip_bounds {
            Some(clip) => {
                let x0 = bounds.x.max(clip.x);
                let y0 = bounds.y.max(clip.y);
                let x1 = bounds.right().min(clip.right());
                let y1 = bounds.bottom().min(clip.bottom());
                Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
            }
            None => bounds,
        })
    } else {
        clip_bounds
    };

    // A *content*-transform node (a `SceneView`'s pan/zoom) places its children
    // in the transformed (content) coordinate space — `place_children` writes
    // each child's `node.bounds` in scene coords, and the pan/zoom is applied
    // only at draw time via the `PushTransform` above. The cull clip we hand the
    // children must therefore be in that same content space; otherwise the
    // per-child offscreen check at the top of this fn compares scene-space child
    // bounds against a screen-space clip and drops content that is panned into
    // view (a card far down in scene coords reads as "outside the viewport"
    // regardless of pan — the lightweight tier, painted in the node's own
    // `paint()`, is unaffected, hence "connectors render but cards don't").
    // Inverse-transform the screen-space clip into content space. The GPU
    // `SetClip` (emitted in parent/screen space) is untouched — it stays the
    // real viewport scissor.
    let next_clip = match (
        content_transform,
        next_clip,
        transform.and_then(|t| t.inverse()),
    ) {
        (true, Some(screen_clip), Some(inv)) => Some(inv.apply_rect(screen_clip)),
        _ => next_clip,
    };

    // Self-transform / plain clipping nodes emit their clip here — after the
    // node's own paint, inside any transform scope. Content-transform nodes
    // already emitted theirs above (in parent space).
    if clips && !clip_outside_transform {
        frame
            .draw_order
            .push(bastyde_canvas::DrawCommand::SetClip(bounds));
    }

    for child_id in children {
        // Skip overlay-managed content here — it is painted via the
        // dedicated overlay loop in render_with_ops. Without this, an
        // overlay (e.g. a tooltip) attached as a child of its anchor
        // would paint twice per frame: once via the parent walk and
        // once via the overlay loop.
        if overlay_skip.contains(&child_id) {
            continue;
        }
        paint_widget_cached(
            arena,
            child_id,
            frame,
            base_theme,
            text_backend,
            next_clip,
            a11y_prefs,
            paint_epoch,
            overlay_skip,
            layout_direction,
            this_effective_enabled,
            this_raster_scale,
        );
    }

    // Post-order `after_paint` hook. Fires after every descendant has
    // painted and committed its bounds, so a parent can read those
    // bounds via `WidgetTreeView::bounds(child_id)`. Gated on
    // `wants_after_paint()` to avoid a virtual call per widget per
    // frame for the 99% of widgets that don't aggregate. The arena
    // mutable borrow from the recursive child loop has dropped by
    // this point, so an immutable reborrow is safe.
    {
        let arena_ref: &WidgetArena = &*arena;
        if let Some(node) = arena_ref.get(id)
            && node.widget.wants_after_paint()
        {
            let view = crate::widget::WidgetTreeView::new(arena_ref);
            let resolved_theme = arena_ref.resolve_theme(id, base_theme);
            let ctx = PaintContext {
                theme: &resolved_theme,
                scale_factor: this_raster_scale,
                layout_direction,
                effective_enabled: this_effective_enabled,
                prefers_high_contrast: a11y_prefs.high_contrast,
                prefers_reduced_motion: a11y_prefs.reduced_motion,
                prefers_large_text: a11y_prefs.large_text,
            };
            node.widget.after_paint(&view, &ctx);
        }
    }

    // Foreground pass — `post_paint` emits *after* the whole child
    // subtree, so its draws land on top of this node's descendants. Still
    // inside this node's clip / transform / opacity / blur scopes (their
    // closers come below), so a foreground decoration pans, scales and
    // clips consistently with the subtree it covers. Same `needs_paint`
    // cache gate as the main paint above, with its own `cached_post_paint`
    // frame. Gated on `wants_post_paint` so non-foreground widgets pay
    // nothing.
    let wants_post_paint = arena
        .get(id)
        .map(|n| n.widget.wants_post_paint())
        .unwrap_or(false);
    if wants_post_paint {
        let has_post_cache = arena
            .get(id)
            .map(|n| n.cached_post_paint.is_some())
            .unwrap_or(false);
        if needs_paint || !has_post_cache {
            // Children restored the backend to this node's ambient scale
            // on their way out; re-assert defensively so post_paint text
            // (e.g. a SceneView's foreground lightweight items) can't
            // bake at a child-leaked scale.
            if raster_scale_changed && let Some(tb) = text_backend {
                tb.borrow_mut().set_raster_scale(this_raster_scale);
            }
            let resolved_theme = arena.resolve_theme(id, base_theme);
            let ctx = PaintContext {
                theme: &resolved_theme,
                scale_factor: this_raster_scale,
                layout_direction,
                effective_enabled: this_effective_enabled,
                prefers_high_contrast: a11y_prefs.high_contrast,
                prefers_reduced_motion: a11y_prefs.reduced_motion,
                prefers_large_text: a11y_prefs.large_text,
            };
            let bounds = arena.bounds(id);
            let node = arena.get(id).expect("node id is active (guarded above)");
            let mut canvas = match text_backend {
                Some(tb) => Canvas::with_text_backend(tb.clone()),
                None => Canvas::new(),
            };
            node.widget.post_paint(bounds, &mut canvas, &ctx);
            let post_frame = canvas.into_render_frame();
            frame.merge(&post_frame);
            if let Some(node) = arena.get_mut(id) {
                node.cached_post_paint = Some(post_frame);
            }
        } else if let Some(cached) = arena.get(id).and_then(|n| n.cached_post_paint.as_ref()) {
            if !cached.layout_keys.is_empty()
                && let Some(tb) = text_backend
            {
                let mut tb = tb.borrow_mut();
                for key in &cached.layout_keys {
                    tb.touch_layout(*key);
                }
                #[cfg(debug_assertions)]
                debug_validate_layout_keys(&*tb, &cached.layout_keys, Some(id));
            }
            frame.merge(cached);
        }
    }

    if clips && !clip_outside_transform {
        frame
            .draw_order
            .push(bastyde_canvas::DrawCommand::ClearClip);
    }

    if push_transform.is_some() {
        frame
            .draw_order
            .push(bastyde_canvas::DrawCommand::PopTransform);
    }

    // A content-transform clip opened before the transform, so it closes after
    // the transform pops (clip { transform { … } } nesting).
    if clip_outside_transform {
        frame
            .draw_order
            .push(bastyde_canvas::DrawCommand::ClearClip);
    }

    if opacity.is_some() {
        frame
            .draw_order
            .push(bastyde_canvas::DrawCommand::RestoreOpacity);
    }

    if blur_radius.is_some() {
        frame
            .draw_order
            .push(bastyde_canvas::DrawCommand::EndBlurredSubtree);
    }

    // Unwind the ambient raster scale to the parent's value so siblings
    // painted after this subtree see their own ancestor scale.
    if raster_scale_changed && let Some(tb) = text_backend {
        tb.borrow_mut().set_raster_scale(accumulated_raster_scale);
    }
}

/// Debug-build corruption catcher: before a retained paint frame is
/// replayed, verify that every layout baked into it still matches the
/// live glyph atlas.
///
/// A `RectMismatch` means the frame's glyph quads reference atlas pixels
/// that were evicted and reused — replaying them draws the wrong
/// characters (the historical "random text corruption fixed by a
/// repaint" bug). That is always a missing cache-invalidation path, so
/// abort with a diagnostic. A `StaleKey` (backend forgot the layout
/// entirely — its caches were cleared after this frame was baked) is the
/// same bug class but can transiently occur around legitimate wholesale
/// clears, so it logs loudly (once per key) instead of aborting.
#[cfg(debug_assertions)]
fn debug_validate_layout_keys(
    tb: &dyn bastyde_canvas::TextBackend,
    layout_keys: &[u64],
    widget: Option<WidgetId>,
) {
    use bastyde_canvas::GlyphValidation;
    std::thread_local! {
        static REPORTED_STALE_KEYS: std::cell::RefCell<std::collections::HashSet<u64>> =
            std::cell::RefCell::new(std::collections::HashSet::new());
    }
    for key in layout_keys {
        match tb.debug_validate_layout(*key) {
            GlyphValidation::Valid => {}
            GlyphValidation::StaleKey => {
                let first_report = REPORTED_STALE_KEYS.with(|set| set.borrow_mut().insert(*key));
                if first_report {
                    eprintln!(
                        "[bastyde] WARNING: retained paint cache replays layout_key={key} \
                         (widget={widget:?}) that the text backend no longer knows — the \
                         frame survived a backend cache clear, so a paint-cache \
                         invalidation path is likely missing."
                    );
                }
            }
            GlyphValidation::RectMismatch => {
                panic!(
                    "stale glyph UVs in retained paint cache (layout_key={key}, \
                     widget={widget:?}): cached quads no longer match the live glyph \
                     atlas — a cache-invalidation path is missing"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_widgets::{FillWidget, StackWidget};
    use bastyde_tokens::{Color, CornerRadius};

    /// Headless text backend that records every `touch_layout` call and
    /// hands out layouts with a fixed non-zero `layout_key`, so tests
    /// can assert that paint-cache reuse keeps glyph timestamps fresh.
    /// Also tracks the ambient raster scale (`set_raster_scale`) and the
    /// scale every `layout_single_line` call ran under, so walker tests
    /// can assert the set/restore discipline around transform scopes.
    #[derive(Default)]
    struct RecordingTextBackend {
        touched: std::rc::Rc<std::cell::RefCell<Vec<u64>>>,
        raster_scale: f32,
        /// Ambient raster scale observed by each `layout_single_line`
        /// call, in call order.
        layout_scales: std::rc::Rc<std::cell::RefCell<Vec<f32>>>,
    }

    impl RecordingTextBackend {
        fn new(touched: std::rc::Rc<std::cell::RefCell<Vec<u64>>>) -> Self {
            Self {
                touched,
                raster_scale: 1.0,
                layout_scales: Default::default(),
            }
        }
    }

    impl bastyde_canvas::TextBackend for RecordingTextBackend {
        fn set_raster_scale(&mut self, raster_scale: f32) {
            self.raster_scale = raster_scale;
        }

        fn raster_scale(&self) -> f32 {
            self.raster_scale
        }

        fn layout_single_line(
            &mut self,
            text: &str,
            _style: &bastyde_tokens::TextStyle,
            _max_width: Option<f32>,
        ) -> bastyde_canvas::TextLayout {
            self.layout_scales.borrow_mut().push(self.raster_scale);
            bastyde_canvas::TextLayout {
                width: text.len() as f32 * 8.0,
                height: 16.0,
                ascent: 12.0,
                descent: 4.0,
                underline_offset: 1.0,
                underline_thickness: 1.0,
                layout_key: 42,
                line_count: 1,
                spans: Vec::new(),
                raster_scale: self.raster_scale,
            }
        }

        fn ensure_glyphs(
            &mut self,
            _layout: &bastyde_canvas::TextLayout,
        ) -> Vec<bastyde_canvas::GlyphQuad> {
            Vec::new()
        }

        fn touch_layout(&mut self, layout_key: u64) {
            self.touched.borrow_mut().push(layout_key);
        }
    }

    /// Leaf widget that draws one line of text so its emitted frame
    /// carries a `layout_key`.
    #[derive(Debug)]
    struct TextPaintWidget;

    impl Widget for TextPaintWidget {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(100.0, 20.0).into()
        }

        fn paint(
            &self,
            bounds: bastyde_canvas::Rect,
            canvas: &mut bastyde_canvas::Canvas,
            _ctx: &PaintContext,
        ) {
            let style = bastyde_tokens::TextStyle::default();
            let _ = canvas.draw_text("hello", bounds, &style, Color::BLACK);
        }
    }

    #[test]
    fn full_frame_cache_hit_touches_layout_keys() {
        let touched = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let backend = RecordingTextBackend::new(touched.clone());
        let mut tree = WidgetTree::new()
            .with_theme(crate::presets::intui::light())
            .with_text_backend(std::rc::Rc::new(std::cell::RefCell::new(backend)));
        tree.add(TextPaintWidget);
        tree.layout(SizeProposal::exact(200.0, 50.0));

        // First render paints for real; no cache reuse yet.
        let _ = tree.render();
        touched.borrow_mut().clear();

        // Nothing is dirty: this render takes the full-frame early-out.
        // The reused frame's layout keys must still be touched, or the
        // backend's LRU ages out glyphs that are on screen.
        let _ = tree.render();
        assert!(
            touched.borrow().contains(&42),
            "full-frame cache hit must touch_layout every baked layout key; \
             touched = {:?}",
            touched.borrow()
        );
    }

    /// Shared fixture: a root stack with a text leaf before, inside, and
    /// after a transform-scoped wrapper:
    ///
    /// ```text
    /// root Stack
    /// ├── TextPaintWidget          (A, screen scale)
    /// ├── Stack [transform]        (wrapper)
    /// │   └── TextPaintWidget      (B, scaled)
    /// └── TextPaintWidget          (C, screen scale)
    /// ```
    fn scaled_subtree_tree(
        transform: impl Into<crate::signal::Prop<bastyde_canvas::Transform2D>>,
    ) -> (
        WidgetTree,
        std::rc::Rc<std::cell::RefCell<RecordingTextBackend>>,
        std::rc::Rc<std::cell::RefCell<Vec<f32>>>,
    ) {
        let backend = RecordingTextBackend::new(Default::default());
        let layout_scales = backend.layout_scales.clone();
        let backend_rc = std::rc::Rc::new(std::cell::RefCell::new(backend));
        let mut tree = WidgetTree::new()
            .with_theme(crate::presets::intui::light())
            .with_text_backend(backend_rc.clone()
                as std::rc::Rc<std::cell::RefCell<dyn bastyde_canvas::TextBackend>>);

        let root = tree.add(StackWidget::new());
        tree.add_child(root, TextPaintWidget); // A
        let wrapper = tree.add_child(root, StackWidget::new());
        tree.add_child(wrapper, TextPaintWidget); // B
        tree.add_child(root, TextPaintWidget); // C
        tree.set_transform(wrapper, transform);

        tree.layout(SizeProposal::exact(400.0, 300.0));
        (tree, backend_rc, layout_scales)
    }

    #[test]
    fn walker_sets_raster_scale_for_scaled_subtree_and_restores() {
        let (mut tree, backend_rc, layout_scales) =
            scaled_subtree_tree(bastyde_canvas::Transform2D::scale(2.0, 2.0));
        let _ = tree.render();

        // Paint order is child order: A (screen), B (inside the 2x
        // scope, quantized onto the 1.25^n ladder), C (screen again —
        // the wrapper restored the ambient scale on exit).
        let expected_b = 1.25_f32.powi(3); // quantize(2.0)
        assert_eq!(
            layout_scales.borrow().as_slice(),
            &[1.0, expected_b, 1.0],
            "text inside the transform scope must lay out at the quantized \
             scale; siblings before/after at screen scale"
        );
        // The walk unwound to the root ambient scale.
        assert_eq!(backend_rc.borrow().raster_scale, 1.0);
    }

    #[test]
    fn raster_scale_change_repaints_clean_descendants() {
        let transform = crate::signal::Signal::new(bastyde_canvas::Transform2D::scale(2.0, 2.0));
        let (mut tree, _backend_rc, layout_scales) = scaled_subtree_tree(transform.clone());
        let _ = tree.render();
        layout_scales.borrow_mut().clear();

        // Zoom changes: only the wrapper node is dirtied (RepaintOnly
        // binding); its text child B stays clean — the raster-scale
        // stamp on B's paint cache must force the repaint anyway.
        transform.set(bastyde_canvas::Transform2D::scale(3.0, 3.0));
        let _ = tree.render();
        assert_eq!(
            layout_scales.borrow().as_slice(),
            &[1.25_f32.powi(5)],
            "the clean text leaf inside the scope must re-rasterize at the \
             new quantized scale (A and C stay cached: no layout calls)"
        );

        // And back down to identity: B re-bakes once more at 1.0.
        layout_scales.borrow_mut().clear();
        transform.set(bastyde_canvas::Transform2D::IDENTITY);
        let _ = tree.render();
        assert_eq!(layout_scales.borrow().as_slice(), &[1.0]);

        // Steady state: nothing dirty, no scale movement → full-frame
        // cache hit, zero layout calls.
        layout_scales.borrow_mut().clear();
        let _ = tree.render();
        assert!(layout_scales.borrow().is_empty());
    }

    #[test]
    fn nested_scale_transforms_multiply() {
        let backend = RecordingTextBackend::new(Default::default());
        let layout_scales = backend.layout_scales.clone();
        let mut tree = WidgetTree::new()
            .with_theme(crate::presets::intui::light())
            .with_text_backend(std::rc::Rc::new(std::cell::RefCell::new(backend)));

        // outer [2x] → inner [1.5x] → text. Accumulation:
        // quantize(2.0) = 1.25^3, then quantize(1.25^3 × 1.5) = 1.25^5.
        let outer = tree.add(StackWidget::new());
        let inner = tree.add_child(outer, StackWidget::new());
        tree.add_child(inner, TextPaintWidget);
        tree.set_transform(outer, bastyde_canvas::Transform2D::scale(2.0, 2.0));
        tree.set_content_transform(inner, bastyde_canvas::Transform2D::scale(1.5, 1.5));

        tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = tree.render();
        assert_eq!(layout_scales.borrow().as_slice(), &[1.25_f32.powi(5)]);
    }

    #[test]
    fn invalidate_all_paints_clears_post_paint_cache() {
        /// Widget that draws chrome in the foreground pass so the walker
        /// populates `cached_post_paint`.
        #[derive(Debug)]
        struct PostPaintWidget;

        impl Widget for PostPaintWidget {
            fn layout_response(
                &self,
                proposal: SizeProposal,
                _ctx: &LayoutContext,
            ) -> crate::widget::LayoutResponse {
                proposal.resolve(50.0, 50.0).into()
            }

            fn wants_post_paint(&self) -> bool {
                true
            }

            fn post_paint(
                &self,
                bounds: bastyde_canvas::Rect,
                canvas: &mut bastyde_canvas::Canvas,
                _ctx: &PaintContext,
            ) {
                canvas.fill_rect(bounds, Color::RED);
            }
        }

        let mut tree = WidgetTree::new().with_theme(crate::presets::intui::light());
        let id = tree.add(PostPaintWidget);
        tree.layout(SizeProposal::exact(100.0, 100.0));
        let _ = tree.render();
        assert!(
            tree.arena
                .get(id)
                .and_then(|n| n.cached_post_paint.as_ref())
                .is_some(),
            "render must populate cached_post_paint for a post-painting widget"
        );

        tree.invalidate_all_paints();
        assert!(
            tree.arena
                .get(id)
                .and_then(|n| n.cached_post_paint.as_ref())
                .is_none(),
            "invalidate_all_paints must clear cached_post_paint too — a retained \
             post-paint frame can hold stale glyph UVs after atlas eviction"
        );
    }

    #[derive(Debug)]
    struct ThemeAwareWidget;

    impl Widget for ThemeAwareWidget {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }

        fn paint(
            &self,
            bounds: bastyde_canvas::Rect,
            canvas: &mut bastyde_canvas::Canvas,
            ctx: &PaintContext,
        ) {
            canvas.fill_rounded_rect(
                bounds,
                bastyde_tokens::CornerRadius::uniform(4.0),
                ctx.theme.colors.accent,
            );
        }
    }

    #[test]
    fn fill_widget_produces_shape_in_frame() {
        let mut tree = WidgetTree::new().with_theme(crate::presets::intui::light());
        tree.add(
            FillWidget::new()
                .background(Color::RED)
                .corner_radius(CornerRadius::uniform(6.0)),
        );
        tree.layout(SizeProposal::exact(100.0, 40.0));
        let frame = tree.render();
        assert_eq!(frame.shapes.len(), 1);
        assert_eq!(
            frame.shapes[0].shape,
            bastyde_canvas::ShapeKind::RoundedRect
        );
    }

    #[test]
    fn empty_tree_renders_empty_frame() {
        let mut tree = WidgetTree::new();
        let frame = tree.render();
        assert!(frame.is_empty());
    }

    #[test]
    fn render_clears_paint_dirty() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().background(Color::RED));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        assert!(tree.needs_paint());
        tree.render();
        assert!(!tree.needs_paint());
    }

    #[test]
    fn dormant_widget_not_rendered() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(
            FillWidget::new()
                .background(Color::RED)
                .corner_radius(CornerRadius::uniform(4.0)),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();
        assert!(!frame.shapes.is_empty());

        tree.set_dormant(widget);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();
        assert!(frame.shapes.is_empty());
    }

    #[test]
    fn dormancy_is_recursive() {
        let mut tree = WidgetTree::new();
        let child = tree.add(
            FillWidget::new()
                .background(Color::RED)
                .corner_radius(CornerRadius::uniform(4.0)),
        );
        let parent = tree.add(StackWidget::new().add_child(child));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        let frame = tree.render();
        assert_eq!(frame.shapes.len(), 1);

        tree.set_dormant(parent);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();
        assert!(frame.shapes.is_empty());

        tree.activate(parent);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();
        assert_eq!(frame.shapes.len(), 1);
    }

    #[test]
    fn set_theme_marks_all_dirty() {
        let mut tree = WidgetTree::new().with_theme(crate::presets::intui::light());
        tree.add(FillWidget::new().background(Color::RED));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.render();

        assert!(!tree.needs_layout());
        assert!(!tree.needs_paint());

        tree.set_theme(crate::presets::intui::dark());
        assert!(tree.needs_layout());
        assert!(tree.needs_paint());
    }

    #[test]
    fn set_theme_changes_rendered_colors() {
        let mut tree = WidgetTree::new().with_theme(crate::presets::intui::light());
        tree.add(ThemeAwareWidget);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let light_frame = tree.render();
        let light_color = light_frame.shapes[0].color;

        tree.set_theme(crate::presets::intui::dark());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let dark_frame = tree.render();
        let dark_color = dark_frame.shapes[0].color;

        assert_ne!(light_color, dark_color);
    }

    #[test]
    fn subtree_theme_override() {
        let mut tree = WidgetTree::new().with_theme(crate::presets::intui::light());
        let parent = tree.add(ThemeAwareWidget);
        let _child = tree.add_child(parent, ThemeAwareWidget);

        tree.set_theme_override(parent, |theme| {
            theme.colors = bastyde_tokens::ColorTokens::dark_default();
        });

        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();

        let dark_accent = bastyde_tokens::ColorTokens::dark_default()
            .accent
            .to_array();
        assert_eq!(frame.shapes[0].color, dark_accent);
        assert_eq!(frame.shapes[1].color, dark_accent);
    }

    #[test]
    fn theme_override_only_affects_subtree() {
        let mut tree = WidgetTree::new().with_theme(crate::presets::intui::light());

        let _unaffected = tree.add(ThemeAwareWidget);
        let overridden = tree.add(ThemeAwareWidget);

        tree.set_theme_override(overridden, |theme| {
            theme.colors = bastyde_tokens::ColorTokens::dark_default();
        });

        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();

        let light_accent = bastyde_tokens::ColorTokens::light_default()
            .accent
            .to_array();
        let dark_accent = bastyde_tokens::ColorTokens::dark_default()
            .accent
            .to_array();

        assert_eq!(frame.shapes[0].color, light_accent);
        assert_eq!(frame.shapes[1].color, dark_accent);
    }

    #[test]
    fn resolved_theme_reflects_overrides() {
        let mut tree = WidgetTree::new().with_theme(crate::presets::intui::light());

        let parent = tree.add(FillWidget::new());
        let child = tree.add_child(parent, FillWidget::new());

        tree.set_theme_override(parent, |theme| {
            theme.colors.accent = Color::RED;
        });

        tree.layout(SizeProposal::exact(100.0, 50.0));

        let parent_theme = tree.resolved_theme(parent);
        assert_eq!(parent_theme.colors.accent, Color::RED);

        let child_theme = tree.resolved_theme(child);
        assert_eq!(child_theme.colors.accent, Color::RED);
    }

    #[test]
    fn opacity_prop_emits_set_and_restore_around_subtree() {
        // A widget with opacity_prop = Some(Static(0.5)) wraps its
        // own paint AND its children's paint inside a SetOpacity /
        // RestoreOpacity pair, so the canvas's stacked-opacity model
        // multiplies through.
        let mut tree = WidgetTree::new().with_theme(crate::presets::intui::light());
        let parent = tree.add(StackWidget::new());
        let _child = tree.add_child(parent, FillWidget::new().background(Color::RED));
        tree.set_opacity(parent, 0.5_f32);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();

        let mut set_count = 0;
        let mut restore_count = 0;
        for cmd in &frame.draw_order {
            match cmd {
                bastyde_canvas::DrawCommand::SetOpacity(v) => {
                    assert!((v - 0.5).abs() < 1e-6, "expected 0.5, got {}", v);
                    set_count += 1;
                }
                bastyde_canvas::DrawCommand::RestoreOpacity => restore_count += 1,
                _ => {}
            }
        }
        assert_eq!(set_count, 1, "draw_order = {:?}", frame.draw_order);
        assert_eq!(restore_count, 1);
    }

    #[test]
    fn opacity_prop_zero_skips_subtree_entirely() {
        // Sub-perceptual opacity (< 1/512) returns early — no
        // SetOpacity, no children draw commands. Saves a blend pass.
        let mut tree = WidgetTree::new().with_theme(crate::presets::intui::light());
        let parent = tree.add(StackWidget::new());
        tree.add_child(parent, FillWidget::new().background(Color::RED));
        tree.set_opacity(parent, 0.0_f32);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();

        for cmd in &frame.draw_order {
            assert!(
                !matches!(cmd, bastyde_canvas::DrawCommand::SetOpacity(_)),
                "fully-transparent subtree should not emit SetOpacity"
            );
        }
        // The red FillWidget child must not appear either.
        assert!(
            !frame
                .shapes
                .iter()
                .any(|s| s.color == Color::RED.to_array()),
            "fully-transparent subtree should not paint its descendants"
        );
    }

    #[test]
    fn transform_prop_emits_push_and_pop_around_subtree() {
        // A widget with transform_prop = Some(Static(scale(2))) wraps
        // both its own paint AND its children's paint inside a
        // PushTransform / PopTransform pair, mirroring the opacity
        // pattern.
        let mut tree = WidgetTree::new().with_theme(crate::presets::intui::light());
        let parent = tree.add(StackWidget::new());
        let _child = tree.add_child(parent, FillWidget::new().background(Color::RED));
        let scale_2x = bastyde_canvas::Transform2D::scale(2.0, 2.0);
        tree.set_transform(parent, scale_2x);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();

        let mut push_count = 0;
        let mut pop_count = 0;
        let mut push_value = None;
        for cmd in &frame.draw_order {
            match cmd {
                bastyde_canvas::DrawCommand::PushTransform(t) => {
                    push_count += 1;
                    push_value = Some(*t);
                }
                bastyde_canvas::DrawCommand::PopTransform => pop_count += 1,
                _ => {}
            }
        }
        assert_eq!(push_count, 1, "draw_order = {:?}", frame.draw_order);
        assert_eq!(pop_count, 1);
        assert_eq!(push_value, Some(scale_2x));
    }

    #[test]
    fn content_transform_clip_wraps_outside_the_transform() {
        // A *content* transform (the SceneView pattern: clips_children + a
        // content transform set via set_content_transform) must emit its clip
        // OUTSIDE the transform — SetClip before PushTransform, ClearClip after
        // PopTransform — so the renderer scissors to the fixed parent-space
        // viewport instead of the pan/zoom-shifted rect. (A *self* transform
        // like Scale keeps the clip inside the transform; see
        // transform_prop_emits_push_and_pop_around_subtree.)
        let mut tree = WidgetTree::new().with_theme(crate::presets::intui::light());
        let parent = tree.add(StackWidget::new());
        tree.add_child(parent, FillWidget::new().background(Color::RED));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        // Set after layout so no rebuild can clear the flags before render.
        tree.set_clips_children(parent, true);
        tree.set_content_transform(parent, bastyde_canvas::Transform2D::translate(50.0, 30.0));
        let frame = tree.render();

        let mut set_clip = None;
        let mut push = None;
        let mut pop = None;
        let mut clear = None;
        for (i, cmd) in frame.draw_order.iter().enumerate() {
            match cmd {
                bastyde_canvas::DrawCommand::SetClip(_) => set_clip = set_clip.or(Some(i)),
                bastyde_canvas::DrawCommand::PushTransform(_) => push = push.or(Some(i)),
                bastyde_canvas::DrawCommand::PopTransform => pop = Some(i),
                bastyde_canvas::DrawCommand::ClearClip => clear = Some(i),
                _ => {}
            }
        }
        let (sc, pt, pop, cc) = (
            set_clip.expect("SetClip emitted"),
            push.expect("PushTransform emitted"),
            pop.expect("PopTransform emitted"),
            clear.expect("ClearClip emitted"),
        );
        assert!(
            sc < pt,
            "clip must open before the transform: SetClip@{sc}, PushTransform@{pt}; order={:?}",
            frame.draw_order
        );
        assert!(
            pop < cc,
            "clip must close after the transform: PopTransform@{pop}, ClearClip@{cc}"
        );
    }

    #[test]
    fn identity_transform_prop_skipped() {
        // transform_prop = Some(Static(IDENTITY)) is a no-op — the
        // walker should NOT emit a PushTransform / PopTransform pair
        // for the rest pose. Saves a flush per identity wrapper per
        // frame (Scale at full visibility, Rotate at angle=0).
        let mut tree = WidgetTree::new().with_theme(crate::presets::intui::light());
        let parent = tree.add(StackWidget::new());
        tree.add_child(parent, FillWidget::new().background(Color::RED));
        tree.set_transform(parent, bastyde_canvas::Transform2D::IDENTITY);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();

        for cmd in &frame.draw_order {
            assert!(
                !matches!(
                    cmd,
                    bastyde_canvas::DrawCommand::PushTransform(_)
                        | bastyde_canvas::DrawCommand::PopTransform
                ),
                "identity transform must not emit a push/pop scope"
            );
        }
    }

    #[test]
    fn transform_scope_paint_order_opacity_outer_transform_inner() {
        // When both opacity_prop AND transform_prop are set on the
        // same node, the framework's contract is opacity OUTER and
        // transform INNER. This pins down the order so future
        // refactors don't silently flip composability.
        let mut tree = WidgetTree::new().with_theme(crate::presets::intui::light());
        let parent = tree.add(StackWidget::new());
        tree.add_child(parent, FillWidget::new().background(Color::RED));
        tree.set_opacity(parent, 0.7_f32);
        tree.set_transform(parent, bastyde_canvas::Transform2D::scale(2.0, 2.0));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();

        // Find the indices of each command kind.
        let mut set_opacity_idx = None;
        let mut push_transform_idx = None;
        let mut pop_transform_idx = None;
        let mut restore_opacity_idx = None;
        for (i, cmd) in frame.draw_order.iter().enumerate() {
            match cmd {
                bastyde_canvas::DrawCommand::SetOpacity(_) => set_opacity_idx = Some(i),
                bastyde_canvas::DrawCommand::PushTransform(_) => push_transform_idx = Some(i),
                bastyde_canvas::DrawCommand::PopTransform => pop_transform_idx = Some(i),
                bastyde_canvas::DrawCommand::RestoreOpacity => restore_opacity_idx = Some(i),
                _ => {}
            }
        }
        let so = set_opacity_idx.expect("SetOpacity emitted");
        let pt = push_transform_idx.expect("PushTransform emitted");
        let popt = pop_transform_idx.expect("PopTransform emitted");
        let ro = restore_opacity_idx.expect("RestoreOpacity emitted");
        assert!(so < pt, "opacity must open before transform: {so} < {pt}");
        assert!(pt < popt, "transform push must precede its pop");
        assert!(
            popt < ro,
            "transform must close before opacity: {popt} < {ro}"
        );
    }

    /// A composing widget that paints a RED backdrop in `paint()`, hosts a
    /// GREEN child, and paints a BLUE foreground in `post_paint()`. Pins the
    /// P-C-AP draw order: backdrop (P) → child (C) → foreground (AP).
    #[derive(Debug)]
    struct Sandwich {
        child: Option<crate::WidgetId>,
    }

    impl Widget for Sandwich {
        fn build(&mut self, ctx: &mut crate::build_context::BuildContext) -> Vec<crate::WidgetId> {
            let c = ctx.add(FillWidget::new().background(Color::GREEN));
            self.child = Some(c);
            vec![c]
        }

        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }

        fn place_children(
            &self,
            bounds: bastyde_canvas::Rect,
            _proposal: SizeProposal,
            children: &mut [crate::widget::WidgetPlacement],
            _ctx: &LayoutContext,
        ) {
            for child in children.iter_mut() {
                child.origin = bounds.origin();
                child.size = bounds.size();
            }
        }

        fn children(&self) -> Vec<crate::WidgetId> {
            self.child.iter().copied().collect()
        }

        fn paint(
            &self,
            bounds: bastyde_canvas::Rect,
            canvas: &mut bastyde_canvas::Canvas,
            _ctx: &PaintContext,
        ) {
            canvas.fill_rounded_rect(bounds, CornerRadius::uniform(0.0), Color::RED);
        }

        fn wants_post_paint(&self) -> bool {
            true
        }

        fn post_paint(
            &self,
            bounds: bastyde_canvas::Rect,
            canvas: &mut bastyde_canvas::Canvas,
            _ctx: &PaintContext,
        ) {
            canvas.fill_rounded_rect(bounds, CornerRadius::uniform(0.0), Color::BLUE);
        }
    }

    #[test]
    fn post_paint_emits_after_children() {
        // P-C-AP ordering: a composing widget's own paint() is a backdrop
        // (before children) and post_paint() is a foreground (after the whole
        // child subtree). Backdrop=RED, child=GREEN, foreground=BLUE; assert
        // RED < GREEN < BLUE in draw_order.
        let mut tree = WidgetTree::new().with_theme(crate::presets::intui::light());
        tree.add(Sandwich { child: None });
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();

        let color_of = |cmd: &bastyde_canvas::DrawCommand| -> Option<[f32; 4]> {
            match cmd {
                bastyde_canvas::DrawCommand::Shape(i) => frame.shapes.get(*i).map(|s| s.color),
                bastyde_canvas::DrawCommand::Decoration(i) => {
                    frame.decorations.get(*i).map(|d| d.color)
                }
                _ => None,
            }
        };
        // Find the first draw whose color is dominated by `dominant` channel.
        let find = |dominant: usize| -> Option<usize> {
            frame.draw_order.iter().position(|cmd| {
                color_of(cmd).is_some_and(|c| {
                    c[dominant] > 0.5 && (0..3).all(|ch| ch == dominant || c[ch] < 0.5)
                })
            })
        };
        let red = find(0).expect("backdrop (RED) painted");
        let green = find(1).expect("child (GREEN) painted");
        let blue = find(2).expect("foreground (BLUE) painted");
        assert!(
            red < green && green < blue,
            "expected backdrop < child < foreground; RED@{red}, GREEN@{green}, BLUE@{blue}; order={:?}",
            frame.draw_order
        );
    }

    #[test]
    fn blur_prop_emits_begin_end_pair_around_subtree() {
        // A widget with blur_prop = Some(Static(8.0)) wraps both its
        // own paint AND its children's paint inside a BeginBlurredSubtree
        // / EndBlurredSubtree pair, mirroring the opacity and transform
        // patterns. The Begin command carries the widget's bounds and
        // the requested radius.
        let mut tree = WidgetTree::new().with_theme(crate::presets::intui::light());
        let parent = tree.add(StackWidget::new());
        let _child = tree.add_child(parent, FillWidget::new().background(Color::RED));
        tree.set_blur(parent, 8.0_f32);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();

        let mut begin_count = 0;
        let mut end_count = 0;
        let mut begin_radius = None;
        for cmd in &frame.draw_order {
            match cmd {
                bastyde_canvas::DrawCommand::BeginBlurredSubtree { radius, .. } => {
                    begin_count += 1;
                    begin_radius = Some(*radius);
                }
                bastyde_canvas::DrawCommand::EndBlurredSubtree => end_count += 1,
                _ => {}
            }
        }
        assert_eq!(begin_count, 1, "draw_order = {:?}", frame.draw_order);
        assert_eq!(end_count, 1);
        assert_eq!(begin_radius, Some(8.0));
    }

    #[test]
    fn blur_prop_subperceptual_radius_skipped() {
        // blur_prop = Some(Static(0.2)) is below the 0.5 threshold —
        // the walker emits no Begin/End pair so animated 0→target
        // patterns have zero per-frame cost when fully off.
        let mut tree = WidgetTree::new().with_theme(crate::presets::intui::light());
        let parent = tree.add(StackWidget::new());
        tree.add_child(parent, FillWidget::new().background(Color::RED));
        tree.set_blur(parent, 0.2_f32);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();

        for cmd in &frame.draw_order {
            assert!(
                !matches!(
                    cmd,
                    bastyde_canvas::DrawCommand::BeginBlurredSubtree { .. }
                        | bastyde_canvas::DrawCommand::EndBlurredSubtree
                ),
                "sub-perceptual blur must not emit Begin/End"
            );
        }
    }

    #[test]
    fn blur_scope_is_outermost_when_combined_with_opacity_and_transform() {
        // Architectural pin: blur is the OUTERMOST scope so it captures
        // the already-faded, already-transformed subtree into the
        // intermediate texture.  Order on enter:
        //   Begin → SetOpacity → PushTransform → ...paint...
        // Order on exit (LIFO):
        //   PopTransform → RestoreOpacity → End
        let mut tree = WidgetTree::new().with_theme(crate::presets::intui::light());
        let parent = tree.add(StackWidget::new());
        tree.add_child(parent, FillWidget::new().background(Color::RED));
        tree.set_blur(parent, 8.0_f32);
        tree.set_opacity(parent, 0.7_f32);
        tree.set_transform(parent, bastyde_canvas::Transform2D::scale(2.0, 2.0));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();

        let mut begin_idx = None;
        let mut set_opacity_idx = None;
        let mut push_transform_idx = None;
        let mut pop_transform_idx = None;
        let mut restore_opacity_idx = None;
        let mut end_idx = None;
        for (i, cmd) in frame.draw_order.iter().enumerate() {
            match cmd {
                bastyde_canvas::DrawCommand::BeginBlurredSubtree { .. } => begin_idx = Some(i),
                bastyde_canvas::DrawCommand::SetOpacity(_) => set_opacity_idx = Some(i),
                bastyde_canvas::DrawCommand::PushTransform(_) => push_transform_idx = Some(i),
                bastyde_canvas::DrawCommand::PopTransform => pop_transform_idx = Some(i),
                bastyde_canvas::DrawCommand::RestoreOpacity => restore_opacity_idx = Some(i),
                bastyde_canvas::DrawCommand::EndBlurredSubtree => end_idx = Some(i),
                _ => {}
            }
        }
        let bg = begin_idx.expect("Begin emitted");
        let so = set_opacity_idx.expect("SetOpacity emitted");
        let pt = push_transform_idx.expect("PushTransform emitted");
        let popt = pop_transform_idx.expect("PopTransform emitted");
        let ro = restore_opacity_idx.expect("RestoreOpacity emitted");
        let en = end_idx.expect("End emitted");
        assert!(bg < so, "blur opens before opacity");
        assert!(so < pt, "opacity opens before transform");
        assert!(pt < popt);
        assert!(popt < ro, "transform closes before opacity");
        assert!(ro < en, "opacity closes before blur");
    }

    #[test]
    fn nested_theme_overrides_compose() {
        let mut tree = WidgetTree::new().with_theme(crate::presets::intui::light());

        let grandparent = tree.add(FillWidget::new());
        let parent = tree.add_child(grandparent, FillWidget::new());
        let child = tree.add_child(parent, FillWidget::new());

        tree.set_theme_override(grandparent, |theme| {
            theme.colors.accent = Color::RED;
        });
        tree.set_theme_override(parent, |theme| {
            theme.colors.text_secondary = Color::GREEN;
        });

        tree.layout(SizeProposal::exact(100.0, 50.0));

        let child_theme = tree.resolved_theme(child);
        assert_eq!(child_theme.colors.accent, Color::RED);
        assert_eq!(child_theme.colors.text_secondary, Color::GREEN);
    }
}
