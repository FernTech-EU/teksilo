//! [`SceneMinimap`] — a small thumbnail of a [`Scene`](crate::Scene)
//! showing all items as dots / rects scaled down, with an overlay
//! highlighting the currently visible viewport rectangle.
//!
//! ## Use
//!
//! ```ignore
//! use bastyde_scene::{Scene, SceneView, SceneMinimap};
//! use bastyde_canvas::Rect;
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
//! VStack::new()
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
//! that maps to "the entire minimap area"), a static `Vec<(Rect, Color)>`
//! of item thumbnails (refreshed by the app whenever items move),
//! and a `Signal<Rect>` for the live viewport rectangle.
//!
//! Apps that want a live "items as they move" minimap rebuild their
//! widget tree on scene mutations or wire a `Signal<Vec<...>>`. The
//! viewport overlay is reactive on its own — the minimap re-paints
//! whenever the SceneView's pan / zoom changes, with no manual
//! plumbing.

use std::rc::Rc;

use bastyde_canvas::{Canvas, Point, Rect, Size, SizeProposal, StrokeStyle};
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{EventContext, LayoutContext, LayoutResponse, PaintContext, Widget};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::Color;

/// A small thumbnail rendering of a [`Scene`](crate::Scene)'s
/// content, with the live viewport rectangle highlighted.
///
/// Paint order: background fill → optional content-bounds outline
/// → item thumbnails (dots / rects) → viewport overlay rect.
pub struct SceneMinimap {
    /// The scene-coord rect that maps to the full minimap drawing
    /// area. Apps typically use `Scene::content_bounds()` or a
    /// hand-picked extent (e.g. `Rect::new(0,0, 10_000, 10_000)`
    /// for a known canvas).
    content_bounds: Rect,
    /// Live viewport rectangle in scene coords. The minimap binds
    /// at `RepaintOnly` so it re-renders whenever the SceneView
    /// pans / zooms.
    viewport_in_scene: Signal<Rect>,
    /// Static snapshot of items + their thumbnail color. Apps
    /// refresh by rebuilding when items move.
    items: Vec<(Rect, Color)>,
    /// Minimap dimensions in widget-local pixels. Defaults to
    /// 200×150.
    size: Size,
    /// Background fill color. Defaults to a translucent white.
    background: Color,
    /// Border around the minimap drawing area. Default 1px black.
    border: Option<(Color, f32)>,
    /// Color of the viewport overlay rectangle. Default semi-
    /// transparent blue stroke + faint fill.
    viewport_color: Color,
    /// Optional outline of the content extent (gives users a sense
    /// of "you're inside this much scene"). Default `None`.
    content_outline: Option<(Color, f32)>,
    /// Optional click handler: fires with the scene-coord
    /// corresponding to the click, plus the standard
    /// `EventContext`. Apps wire this to `SceneView::pan_to_center`
    /// for click-to-recenter behavior.
    on_click: Option<Rc<dyn Fn(Point, &mut EventContext)>>,
}

impl std::fmt::Debug for SceneMinimap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneMinimap")
            .field("content_bounds", &self.content_bounds)
            .field("size", &self.size)
            .field("item_count", &self.items.len())
            .field("on_click", &self.on_click.is_some())
            .finish_non_exhaustive()
    }
}

impl SceneMinimap {
    /// Construct a minimap covering `content_bounds` (the scene-coord
    /// extent that maps to the full minimap area), with `viewport`
    /// driving the live overlay rectangle.
    pub fn new(content_bounds: Rect, viewport: Signal<Rect>) -> Self {
        Self {
            content_bounds,
            viewport_in_scene: viewport,
            items: Vec::new(),
            size: Size::new(200.0, 150.0),
            background: Color::new(1.0, 1.0, 1.0, 0.85),
            border: Some((Color::new(0.0, 0.0, 0.0, 0.5), 1.0)),
            viewport_color: Color::new(0.2, 0.5, 1.0, 1.0),
            content_outline: None,
            on_click: None,
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
    pub fn items(mut self, items: Vec<(Rect, Color)>) -> Self {
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
    pub fn content_outline(mut self, outline: Option<(Color, f32)>) -> Self {
        self.content_outline = outline;
        self
    }

    /// Click handler: fires with the scene-coord corresponding to
    /// the click, plus the standard `EventContext`. Apps wire this
    /// to e.g. `SceneView::pan_to_center` for click-to-recenter.
    pub fn on_click<F>(mut self, callback: F) -> Self
    where
        F: Fn(Point, &mut EventContext) + 'static,
    {
        self.on_click = Some(Rc::new(callback));
        self
    }

    /// Map a scene-coord point through the minimap's projection to
    /// minimap-local widget coords. Internal helper, exposed for
    /// tests.
    fn scene_to_minimap(&self, scene_pt: Point, area: Rect) -> Point {
        let cb = self.content_bounds;
        let nx = if cb.width > 0.0 {
            (scene_pt.x - cb.x) / cb.width
        } else {
            0.5
        };
        let ny = if cb.height > 0.0 {
            (scene_pt.y - cb.y) / cb.height
        } else {
            0.5
        };
        Point::new(area.x + nx * area.width, area.y + ny * area.height)
    }

    /// Map a minimap-local widget point back to scene coords.
    /// Inverse of `scene_to_minimap`. Currently used only by tests
    /// — kept as inherent (rather than test-only) so apps writing
    /// custom minimap overlays can reuse the projection math.
    #[allow(dead_code)]
    fn minimap_to_scene(&self, mm_pt: Point, area: Rect) -> Point {
        let cb = self.content_bounds;
        let nx = if area.width > 0.0 {
            (mm_pt.x - area.x) / area.width
        } else {
            0.5
        };
        let ny = if area.height > 0.0 {
            (mm_pt.y - area.y) / area.height
        } else {
            0.5
        };
        Point::new(cb.x + nx * cb.width, cb.y + ny * cb.height)
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

        if let Some(callback) = self.on_click.clone() {
            // `on_tap` hands us a widget-local `Point`; project
            // through the minimap mapping into scene coords and
            // dispatch.
            let content = self.content_bounds;
            let size = self.size;
            let handlers = HandlerSet::new().on_tap(move |event, ev_ctx| {
                let local = event.position;
                let nx = if size.width > 0.0 {
                    local.x / size.width
                } else {
                    0.5
                };
                let ny = if size.height > 0.0 {
                    local.y / size.height
                } else {
                    0.5
                };
                let scene_pt = Point::new(
                    content.x + nx * content.width,
                    content.y + ny * content.height,
                );
                callback(scene_pt, ev_ctx);
            });
            ctx.apply_self_handlers(handlers);
        }
        Vec::new()
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
        let area = Rect::new(0.0, 0.0, bounds.width, bounds.height);
        // Background fill.
        canvas.fill_rect(area, self.background);
        // Optional content outline (in minimap-local coords this is
        // a sub-rect mapped from content_bounds; for the full-area
        // mapping that's exactly `area`).
        if let Some((color, width)) = self.content_outline {
            canvas.stroke_rect(area, color, StrokeStyle::solid(width));
        }
        // Item thumbnails.
        for (item_rect, color) in &self.items {
            let tl = self.scene_to_minimap(Point::new(item_rect.x, item_rect.y), area);
            let br = self.scene_to_minimap(
                Point::new(
                    item_rect.x + item_rect.width,
                    item_rect.y + item_rect.height,
                ),
                area,
            );
            let r = Rect::new(tl.x, tl.y, (br.x - tl.x).max(1.0), (br.y - tl.y).max(1.0));
            canvas.fill_rect(r, *color);
        }
        // Viewport overlay.
        let vp = self.viewport_in_scene.get();
        let tl = self.scene_to_minimap(Point::new(vp.x, vp.y), area);
        let br = self.scene_to_minimap(Point::new(vp.x + vp.width, vp.y + vp.height), area);
        let vp_rect = Rect::new(tl.x, tl.y, (br.x - tl.x).max(1.0), (br.y - tl.y).max(1.0));
        canvas.stroke_rect(vp_rect, self.viewport_color, StrokeStyle::solid(2.0));
        // Border last so it sits on top of everything.
        if let Some((color, width)) = self.border {
            canvas.stroke_rect(area, color, StrokeStyle::solid(width));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::widget_tree::WidgetTree;

    #[test]
    fn minimap_default_layout_response_is_capped() {
        let viewport = Signal::new(Rect::new(0.0, 0.0, 100.0, 75.0));
        let mm = SceneMinimap::new(Rect::new(0.0, 0.0, 1000.0, 750.0), viewport);
        let theme = bastyde_core::presets::intui::light();
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
        let theme = bastyde_core::presets::intui::light();
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
    fn scene_to_minimap_maps_corners_correctly() {
        let viewport = Signal::new(Rect::new(0.0, 0.0, 100.0, 75.0));
        let mm = SceneMinimap::new(Rect::new(0.0, 0.0, 1000.0, 750.0), viewport);
        let area = Rect::new(0.0, 0.0, 200.0, 150.0);
        // (0,0) scene → (0,0) minimap
        let p0 = mm.scene_to_minimap(Point::new(0.0, 0.0), area);
        assert!((p0.x - 0.0).abs() < 1e-5);
        assert!((p0.y - 0.0).abs() < 1e-5);
        // (1000, 750) scene → (200, 150) minimap
        let p1 = mm.scene_to_minimap(Point::new(1000.0, 750.0), area);
        assert!((p1.x - 200.0).abs() < 1e-5);
        assert!((p1.y - 150.0).abs() < 1e-5);
        // (500, 375) scene → (100, 75) minimap (center)
        let pc = mm.scene_to_minimap(Point::new(500.0, 375.0), area);
        assert!((pc.x - 100.0).abs() < 1e-5);
        assert!((pc.y - 75.0).abs() < 1e-5);
    }

    #[test]
    fn minimap_to_scene_is_inverse() {
        let viewport = Signal::new(Rect::new(0.0, 0.0, 100.0, 75.0));
        let mm = SceneMinimap::new(Rect::new(50.0, 100.0, 1000.0, 750.0), viewport);
        let area = Rect::new(0.0, 0.0, 200.0, 150.0);
        for (sx, sy) in [(50.0, 100.0), (1050.0, 850.0), (550.0, 475.0)] {
            let mm_pt = mm.scene_to_minimap(Point::new(sx, sy), area);
            let back = mm.minimap_to_scene(mm_pt, area);
            assert!((back.x - sx).abs() < 1e-3);
            assert!((back.y - sy).abs() < 1e-3);
        }
    }
}
