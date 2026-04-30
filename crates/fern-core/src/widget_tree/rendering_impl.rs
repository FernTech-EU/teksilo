use super::*;

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
    /// flush). Called by `fern-app` during its paint pipeline.
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
        if !self.arena.any_needs_paint()
            && let Some(cached) = self.cached_frame.as_mut()
        {
            let frame = std::rc::Rc::make_mut(cached);
            if has_animations {
                let src = self.animated_quads.scratch_slice();
                frame.anim_params.clear();
                frame.anim_params.extend_from_slice(src);
            }
            return std::rc::Rc::clone(cached);
        }

        self.paint_epoch = self.paint_epoch.saturating_add(1);
        let paint_epoch = self.paint_epoch;

        let mut frame = RenderFrame::new();
        let base_theme = self.theme.clone();
        let text_backend = self.text_backend.clone();
        let a11y_prefs = A11yPaintPrefs {
            high_contrast: self.prefers_high_contrast,
            reduced_motion: self.prefers_reduced_motion,
            large_text: self.text_scale_factor > 1.0,
        };

        for root_id in self.arena.roots() {
            paint_widget_cached(
                &mut self.arena,
                root_id,
                &mut frame,
                &base_theme,
                &text_backend,
                None,
                &a11y_prefs,
                paint_epoch,
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
            );
        }

        for id in self.arena.active_ids() {
            if let Some(node) = self.arena.get_mut(id) {
                node.dirty.needs_paint = false;
            }
        }

        if has_animations {
            frame
                .anim_params
                .extend_from_slice(self.animated_quads.scratch_slice());
        }
        frame.debug_validate_stacks();
        let rc = std::rc::Rc::new(frame);
        self.cached_frame = Some(std::rc::Rc::clone(&rc));
        rc
    }
}

/// Recursive paint pass with per-widget caching.
/// Only re-runs `paint()` for widgets with `needs_paint` set; clean widgets
/// reuse their `cached_paint` output. The tree walk still runs for clip/child
/// ordering, but skips the expensive `paint()` call for clean widgets.
#[allow(clippy::too_many_arguments)]
fn paint_widget_cached(
    arena: &mut WidgetArena,
    id: WidgetId,
    frame: &mut RenderFrame,
    base_theme: &fern_tokens::Theme,
    text_backend: &Option<Rc<RefCell<dyn fern_canvas::TextBackend>>>,
    clip_bounds: Option<Rect>,
    a11y_prefs: &A11yPaintPrefs,
    paint_epoch: u64,
) {
    if !arena.is_active(id) {
        return;
    }

    let node = arena.get(id).unwrap();
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
    let node = arena.get(id).unwrap();
    let opacity = node.opacity_prop.as_ref().map(|p| p.get().clamp(0.0, 1.0));
    if let Some(o) = opacity
        && o < 1.0 / 512.0
    {
        // Sub-perceptual: skip the subtree entirely. Saves a draw
        // pass when a `Fade` is fully transparent (e.g. just-dismissed
        // tooltip waiting for cleanup) and prevents 0.0 from emitting
        // a spurious blend pass on the GPU.
        return;
    }
    if let Some(o) = opacity {
        frame
            .draw_order
            .push(fern_canvas::DrawCommand::SetOpacity(o));
    }

    let node = arena.get(id).unwrap();
    let needs_paint = node.dirty.needs_paint;

    if needs_paint || node.cached_paint.is_none() {
        let resolved_theme = arena.resolve_theme(id, base_theme);
        let ctx = PaintContext {
            theme: &resolved_theme,
            scale_factor: 1.0,
            prefers_high_contrast: a11y_prefs.high_contrast,
            prefers_reduced_motion: a11y_prefs.reduced_motion,
            prefers_large_text: a11y_prefs.large_text,
        };

        let bounds = arena.bounds(id);
        let node = arena.get(id).unwrap();

        let mut canvas = match text_backend {
            Some(tb) => Canvas::with_text_backend(tb.clone()),
            None => Canvas::new(),
        };
        node.widget.paint(bounds, &mut canvas, &ctx);
        let widget_frame = canvas.into_render_frame();

        frame.merge(&widget_frame);
        if let Some(node) = arena.get_mut(id) {
            node.cached_paint = Some(widget_frame);
        }
    } else {
        let node = arena.get(id).unwrap();
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
            }
            frame.merge(cached);
        }
    }

    let node = arena.get(id).unwrap();
    let clips = node.clips_children;
    let children: Vec<WidgetId> = node.children.clone();
    let bounds = node.bounds;
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

    if clips {
        frame
            .draw_order
            .push(fern_canvas::DrawCommand::SetClip(bounds));
    }

    for child_id in children {
        paint_widget_cached(
            arena,
            child_id,
            frame,
            base_theme,
            text_backend,
            next_clip,
            a11y_prefs,
            paint_epoch,
        );
    }

    if clips {
        frame.draw_order.push(fern_canvas::DrawCommand::ClearClip);
    }

    if opacity.is_some() {
        frame
            .draw_order
            .push(fern_canvas::DrawCommand::RestoreOpacity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_widgets::{FillWidget, StackWidget};
    use fern_tokens::{Color, CornerRadius, Theme};

    #[derive(Debug)]
    struct ThemeAwareWidget;

    impl Widget for ThemeAwareWidget {
        fn size_that_fits(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> fern_canvas::Size {
            proposal.resolve(0.0, 0.0)
        }

        fn paint(
            &self,
            bounds: fern_canvas::Rect,
            canvas: &mut fern_canvas::Canvas,
            ctx: &PaintContext,
        ) {
            canvas.fill_rounded_rect(
                bounds,
                fern_tokens::CornerRadius::uniform(4.0),
                ctx.theme.colors.accent,
            );
        }
    }

    #[test]
    fn fill_widget_produces_shape_in_frame() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(
            FillWidget::new()
                .background(Color::RED)
                .corner_radius(CornerRadius::uniform(6.0)),
        );
        tree.layout(SizeProposal::exact(100.0, 40.0));
        let frame = tree.render();
        assert_eq!(frame.shapes.len(), 1);
        assert_eq!(frame.shapes[0].shape, fern_canvas::ShapeKind::RoundedRect);
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
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(FillWidget::new().background(Color::RED));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.render();

        assert!(!tree.needs_layout());
        assert!(!tree.needs_paint());

        tree.set_theme(Theme::dark_default());
        assert!(tree.needs_layout());
        assert!(tree.needs_paint());
    }

    #[test]
    fn set_theme_changes_rendered_colors() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(ThemeAwareWidget);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let light_frame = tree.render();
        let light_color = light_frame.shapes[0].color;

        tree.set_theme(Theme::dark_default());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let dark_frame = tree.render();
        let dark_color = dark_frame.shapes[0].color;

        assert_ne!(light_color, dark_color);
    }

    #[test]
    fn subtree_theme_override() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let parent = tree.add(ThemeAwareWidget);
        let _child = tree.add_child(parent, ThemeAwareWidget);

        tree.set_theme_override(parent, |theme| {
            theme.colors = fern_tokens::ColorTokens::dark_default();
        });

        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();

        let dark_accent = fern_tokens::ColorTokens::dark_default().accent.to_array();
        assert_eq!(frame.shapes[0].color, dark_accent);
        assert_eq!(frame.shapes[1].color, dark_accent);
    }

    #[test]
    fn theme_override_only_affects_subtree() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());

        let _unaffected = tree.add(ThemeAwareWidget);
        let overridden = tree.add(ThemeAwareWidget);

        tree.set_theme_override(overridden, |theme| {
            theme.colors = fern_tokens::ColorTokens::dark_default();
        });

        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();

        let light_accent = fern_tokens::ColorTokens::light_default().accent.to_array();
        let dark_accent = fern_tokens::ColorTokens::dark_default().accent.to_array();

        assert_eq!(frame.shapes[0].color, light_accent);
        assert_eq!(frame.shapes[1].color, dark_accent);
    }

    #[test]
    fn resolved_theme_reflects_overrides() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());

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
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let parent = tree.add(StackWidget::new());
        let _child = tree.add_child(parent, FillWidget::new().background(Color::RED));
        tree.set_opacity(parent, 0.5_f32);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();

        let mut set_count = 0;
        let mut restore_count = 0;
        for cmd in &frame.draw_order {
            match cmd {
                fern_canvas::DrawCommand::SetOpacity(v) => {
                    assert!((v - 0.5).abs() < 1e-6, "expected 0.5, got {}", v);
                    set_count += 1;
                }
                fern_canvas::DrawCommand::RestoreOpacity => restore_count += 1,
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
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let parent = tree.add(StackWidget::new());
        tree.add_child(parent, FillWidget::new().background(Color::RED));
        tree.set_opacity(parent, 0.0_f32);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();

        for cmd in &frame.draw_order {
            assert!(
                !matches!(cmd, fern_canvas::DrawCommand::SetOpacity(_)),
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
    fn nested_theme_overrides_compose() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());

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
