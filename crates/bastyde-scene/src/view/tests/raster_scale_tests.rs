//! Raster-scale integration: text painted inside a zoomed `SceneView`
//! must lay out under the quantized ambient raster scale (set by the
//! paint walker from the view's content-transform scope), so its glyphs
//! rasterize densely enough for the zoom that will stretch them. Layout
//! metrics stay logical — zoom never reflows scene text.

use super::*;
use crate::items::TextItem;
use std::cell::RefCell;
use std::rc::Rc;

/// Headless backend recording the ambient raster scale every
/// `layout_single_line` call ran under (the default `layout_paragraph`
/// delegates here, so `TextItem::paint` → `Canvas::draw_paragraph`
/// lands in this recorder too).
#[derive(Default)]
struct ScaleRecordingBackend {
    raster_scale: f32,
    layout_scales: Rc<RefCell<Vec<f32>>>,
}

impl bastyde_canvas::TextBackend for ScaleRecordingBackend {
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
            layout_key: 7,
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
}

/// Build a tree holding a `SceneView` over a scene with one lightweight
/// `TextItem`, backed by a scale-recording text backend. Returns the
/// tree, the view id, and the recorded per-layout scales.
fn text_scene_tree() -> (WidgetTree, WidgetId, Rc<RefCell<Vec<f32>>>) {
    let backend = ScaleRecordingBackend {
        raster_scale: 1.0,
        ..Default::default()
    };
    let layout_scales = backend.layout_scales.clone();

    let mut scene = Scene::new();
    scene.add_item(
        TextItem::new(lit!("Hello scene"), Rect::new(0.0, 0.0, 200.0, 30.0)),
        Point::new(20.0, 20.0),
    );

    let mut tree = WidgetTree::new()
        .with_text_backend(Rc::new(RefCell::new(backend)));
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    (tree, view_id, layout_scales)
}

#[test]
fn scene_text_lays_out_at_quantized_zoom_scale() {
    let (mut tree, view_id, layout_scales) = text_scene_tree();

    // Zoom 1.0: the view transform is identity-scaled, text lays out at
    // the root ambient scale.
    let _ = tree.render();
    assert_eq!(layout_scales.borrow().as_slice(), &[1.0]);

    // Zoom to 2.0: the walker derives the ambient scale from the view's
    // content transform and the item re-rasterizes at the nearest
    // 1.25^n bucket. Layout runs again but the metrics are logical, so
    // nothing reflows — only the recorded scale moves.
    layout_scales.borrow_mut().clear();
    view_handle(&tree, view_id).set_zoom(2.0);
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let _ = tree.render();
    assert_eq!(
        layout_scales.borrow().as_slice(),
        &[1.25_f32.powi(3)],
        "scene text must re-lay out exactly once, at the quantized zoom scale"
    );

    // And back to 1.0.
    layout_scales.borrow_mut().clear();
    view_handle(&tree, view_id).set_zoom(1.0);
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let _ = tree.render();
    assert_eq!(layout_scales.borrow().as_slice(), &[1.0]);
}
