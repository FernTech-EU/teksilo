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
    pub fn render(&mut self) -> RenderFrame {
        self.process_state_changes();

        if !self.arena.any_needs_paint()
            && let Some(ref cached) = self.cached_frame
        {
            return cached.clone();
        }

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
            );
        }

        for id in self.arena.active_ids() {
            if let Some(node) = self.arena.get_mut(id) {
                node.dirty.needs_paint = false;
            }
        }

        frame.debug_validate_stacks();
        self.cached_frame = Some(frame.clone());
        frame
    }
}

/// Recursive paint pass with per-widget caching.
/// Only re-runs `paint()` for widgets with `needs_paint` set; clean widgets
/// reuse their `cached_paint` output. The tree walk still runs for clip/child
/// ordering, but skips the expensive `paint()` call for clean widgets.
fn paint_widget_cached(
    arena: &mut WidgetArena,
    id: WidgetId,
    frame: &mut RenderFrame,
    base_theme: &fern_tokens::Theme,
    text_backend: &Option<Rc<RefCell<dyn fern_canvas::TextBackend>>>,
    clip_bounds: Option<Rect>,
    a11y_prefs: &A11yPaintPrefs,
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
            return;
        }
    }

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
        );
    }

    if clips {
        frame.draw_order.push(fern_canvas::DrawCommand::ClearClip);
    }
}