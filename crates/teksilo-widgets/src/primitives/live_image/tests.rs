// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! LiveImage in a tree: its box (spec C.1, C.2, C.7), what a commit costs
//! (C.3-C.6), placement and mapping through the real widget (C.10-C.12),
//! accessibility (C.13, C.14), release and source switches (C.15), the
//! device-scale relayout (B.9), snapping under ancestors' transforms, the
//! letterbox, the placeholder, the `Debug` repr and the handle's lifetime.
//! Pixels go through the renderer's own live pass with textures in memory.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use teksilo_canvas::live_image::testing::{CountingWaker, LiveImageMirror};
use teksilo_canvas::wake::{RedrawWaker, WakeKind};
use teksilo_canvas::{DrawCommand, MockTextBackend, RenderFrame, TextBackend};
use teksilo_core::accesskit::Role;
use teksilo_core::event::{EventResponse, ScrollDelta, WidgetEvent};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_core::{Modifiers, PointerButton};

use super::*;
use crate::primitives::{Center, HStack, Padding};

/// A frame whose every pixel names its position and `seed`.
fn pattern(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            px.extend_from_slice(&[x as u8, y as u8, seed, 255]);
        }
    }
    px
}

/// A source with one `w × h` frame committed, and its writer.
fn live(w: u32, h: u32) -> (LiveImageSource, LiveImageWriter) {
    let source = LiveImageSource::new(LivePixelFormat::Rgba8);
    let writer = source.writer();
    writer
        .write_frame(w, h, &pattern(w, h, 0), (w * 4) as usize)
        .unwrap();
    (source, writer)
}

fn commit(writer: &LiveImageWriter, w: u32, h: u32, seed: u8) {
    writer
        .write_frame(w, h, &pattern(w, h, seed), (w * 4) as usize)
        .unwrap();
}

fn tree() -> WidgetTree {
    let backend: Rc<RefCell<dyn TextBackend>> = Rc::new(RefCell::new(MockTextBackend::new()));
    WidgetTree::new()
        .with_theme(teksilo_core::presets::intui::light())
        .with_text_backend(backend)
}

/// The box `widget` asks for under `proposal` at device scale `scale`.
fn ask(widget: &LiveImage, proposal: SizeProposal, scale: f32) -> LayoutResponse {
    let theme = teksilo_core::presets::intui::light();
    let mut ctx = LayoutContext::for_testing(&theme);
    ctx.scale_factor = scale;
    widget.layout_response(proposal, &ctx)
}

fn size_of(widget: &LiveImage, proposal: SizeProposal, scale: f32) -> (f32, f32) {
    let size = ask(widget, proposal, scale).size;
    (size.width, size.height)
}

fn close(a: (f32, f32), b: (f32, f32)) -> bool {
    (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3
}

/// Counts its paints.
#[derive(Debug)]
struct Painter(Rc<Cell<u32>>);

impl Widget for Painter {
    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(20.0, 20.0).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        self.0.set(self.0.get() + 1);
        canvas.fill_rect(bounds, Color::from_rgb(0.2, 0.3, 0.4));
    }
}

/// Applies a fixed transform to its one child, as `Scale` and `Rotate` do.
#[derive(Debug)]
struct Transformed {
    transform: Transform2D,
    child: Option<LiveImage>,
    child_id: Option<WidgetId>,
}

impl Transformed {
    fn new(transform: Transform2D, child: LiveImage) -> Self {
        Self {
            transform,
            child: Some(child),
            child_id: None,
        }
    }
}

impl Widget for Transformed {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        ctx.set_transform(id, self.transform);
        let child = ctx.add(self.child.take().expect("built once"));
        self.child_id = Some(child);
        vec![child]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}

/// The frame's one live quad, and the draw command that names it.
fn only_quad(frame: &RenderFrame) -> &teksilo_canvas::live_image::LiveImageQuad {
    assert_eq!(frame.live_images.len(), 1, "one live quad");
    assert!(
        frame
            .draw_order
            .iter()
            .any(|c| matches!(c, DrawCommand::LiveImage(0))),
        "and its draw command"
    );
    &frame.live_images[0]
}

// ── C.1, C.2, C.7: the box ──

#[test]
fn c1_aspect_is_the_largest_box_of_the_pictures_ratio() {
    let (portrait, _w) = live(720, 1280);
    let image = LiveImage::new(portrait).alt("x");
    let at = SizeProposal::exact(864.0, 754.0);
    // s = 754 / 1280 = 0.5890625: 424.125 x 754, rounded to 424 x 754.
    assert!(close(size_of(&image, at, 1.0), (424.0, 754.0)));
    let (landscape, _w) = live(1280, 720);
    let image = LiveImage::new(landscape).alt("x");
    assert!(close(size_of(&image, at, 1.0), (864.0, 486.0)));
    assert_eq!(ask(&image, at, 1.0).flex, 0.0, "rigid");

    // An open axis follows the bounded one; both open is the natural size.
    let (wide, _w) = live(100, 50);
    let image = LiveImage::new(wide).alt("x");
    assert!(close(
        size_of(&image, SizeProposal::with_width(200.0), 1.0),
        (200.0, 100.0)
    ));
    assert!(close(
        size_of(&image, SizeProposal::unspecified(), 1.0),
        (100.0, 50.0)
    ));
    // It scales up as well as down.
    assert!(close(
        size_of(&image, SizeProposal::exact(1000.0, 1000.0), 1.0),
        (1000.0, 500.0)
    ));
}

#[test]
fn c1_aspect_rounds_to_device_pixels_inside_the_proposals_grid() {
    // 100.6 x 1.006 rounds to 101 x 1, then the width is kept inside
    // floor(100.6) = 100.
    let (strip, _w) = live(1000, 10);
    let image = LiveImage::new(strip).alt("x");
    assert!(close(
        size_of(&image, SizeProposal::exact(100.6, 50.0), 1.0),
        (100.0, 1.0)
    ));
    // 5 x 2.142857: at 1.0 the height rounds to 2.
    let (frame, _w) = live(7, 3);
    let image = LiveImage::new(frame).alt("x");
    let at = SizeProposal::exact(5.0, 100.0);
    assert!(close(size_of(&image, at, 1.0), (5.0, 2.0)));
    // At 1.25 both sides round: 6.25 device pixels wide is 6, so 4.8.
    assert!(close(size_of(&image, at, 1.25), (4.8, 2.4)));
}

#[test]
fn c1_without_a_frame_size_aspect_and_fill_take_the_bounded_axes() {
    let empty = LiveImageSource::new(LivePixelFormat::Rgba8);
    let image = LiveImage::new(empty.clone()).alt("x");
    assert!(close(
        size_of(&image, SizeProposal::exact(300.0, 200.0), 1.0),
        (300.0, 200.0)
    ));
    assert!(close(
        size_of(&image, SizeProposal::with_width(300.0), 1.0),
        (300.0, 0.0)
    ));
    assert!(close(
        size_of(&image, SizeProposal::unspecified(), 1.0),
        (0.0, 0.0)
    ));
    let fill = LiveImage::new(empty.clone())
        .sizing(LiveImageSizing::Fill)
        .alt("x");
    let response = ask(&fill, SizeProposal::with_width(300.0), 1.0);
    assert!(close(
        (response.size.width, response.size.height),
        (300.0, 0.0)
    ));
    assert_eq!(response.flex, 1.0);
    let natural = LiveImage::new(empty)
        .sizing(LiveImageSizing::Natural)
        .alt("x");
    assert!(close(
        size_of(&natural, SizeProposal::exact(300.0, 200.0), 1.0),
        (0.0, 0.0)
    ));
}

#[test]
fn c1_a_size_hint_is_the_frame_size_before_the_first_frame() {
    let hinted = LiveImageSource::builder(LivePixelFormat::Rgba8)
        .size_hint(100, 50)
        .build();
    let image = LiveImage::new(hinted).alt("x");
    assert!(close(
        size_of(&image, SizeProposal::exact(400.0, 400.0), 1.0),
        (400.0, 200.0)
    ));
}

#[test]
fn c1_fill_takes_the_proposal_and_grows() {
    let (frame, _w) = live(100, 50);
    let fill = LiveImage::new(frame).sizing(LiveImageSizing::Fill).alt("x");
    let response = ask(&fill, SizeProposal::exact(300.0, 300.0), 1.0);
    assert!(close(
        (response.size.width, response.size.height),
        (300.0, 300.0)
    ));
    assert_eq!(response.flex, 1.0);
    // An open axis follows the picture.
    assert!(close(
        size_of(&fill, SizeProposal::with_width(300.0), 1.0),
        (300.0, 150.0)
    ));
}

#[test]
fn c1_pins_win_over_the_mode() {
    let (frame, _w) = live(100, 50);
    let empty = LiveImageSource::new(LivePixelFormat::Rgba8);
    let at = SizeProposal::exact(800.0, 800.0);
    for sizing in [
        LiveImageSizing::Aspect,
        LiveImageSizing::Fill,
        LiveImageSizing::Natural,
    ] {
        let pinned = |source: &LiveImageSource| LiveImage::new(source.clone()).sizing(sizing);
        assert!(close(
            size_of(&pinned(&frame).size(40.0, 30.0), at, 1.0),
            (40.0, 30.0)
        ));
        assert!(close(
            size_of(&pinned(&empty).size(40.0, 30.0), at, 1.0),
            (40.0, 30.0)
        ));
        assert!(close(
            size_of(&pinned(&frame).width(200.0), at, 1.0),
            (200.0, 100.0)
        ));
        assert!(close(
            size_of(&pinned(&frame).height(100.0), at, 1.0),
            (200.0, 100.0)
        ));
        assert!(close(
            size_of(&pinned(&empty).width(200.0), at, 1.0),
            (200.0, 0.0)
        ));
        assert!(close(
            size_of(&pinned(&empty).height(100.0), at, 1.0),
            (0.0, 100.0)
        ));
        assert_eq!(ask(&pinned(&frame).width(200.0), at, 1.0).flex, 0.0);
    }
}

#[test]
fn c2_natural_is_one_source_pixel_per_logical_or_device_pixel() {
    let (frame, _w) = live(720, 1280);
    let at = SizeProposal::exact(100.0, 100.0);
    let natural = LiveImage::new(frame.clone())
        .sizing(LiveImageSizing::Natural)
        .alt("x");
    assert!(close(size_of(&natural, at, 2.0), (720.0, 1280.0)));
    let device = LiveImage::new(frame)
        .sizing(LiveImageSizing::Natural)
        .device_pixels(true)
        .alt("x");
    assert!(close(size_of(&device, at, 2.0), (360.0, 640.0)));
    // At 1.25, 720 device pixels are 576 logical ones.
    assert!(close(size_of(&device, at, 1.25), (576.0, 1024.0)));
}

/// The picture is fitted from the natural size its box is measured from.
/// With `device_pixels` at scale 2, an 8 × 8 source is 4 × 4 logical
/// pixels: `None` draws all of it there, at one texel a device pixel, and
/// `ScaleDown` never grows it past that in a larger box. It used to be
/// fitted from its 8 × 8 pixels: `None` drew its central quarter, twice the
/// size, and `ScaleDown` drew it at twice its device-pixel size.
#[test]
fn c2_a_picture_measured_in_device_pixels_is_fitted_at_that_size() {
    let laid_out = |image: LiveImage, proposal: SizeProposal| {
        let handle = image.handle();
        let mut tree = tree();
        tree.set_device_scale_factor(2.0);
        let id = tree.add(image);
        tree.add(HStack::new().child(id));
        tree.layout(proposal);
        let frame = tree.render();
        (handle.geometry().unwrap(), only_quad(&frame).uv)
    };
    let image = |fit| {
        LiveImage::new(live(8, 8).0)
            .device_pixels(true)
            .fit(fit)
            .scaling(ScalingFilter::Nearest)
            .alt("x")
    };
    let whole = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

    let (g, uv) = laid_out(
        image(ImageFit::None).sizing(LiveImageSizing::Natural),
        SizeProposal::exact(50.0, 50.0),
    );
    assert_eq!(g.bounds, Rect::new(0.0, 0.0, 4.0, 4.0));
    assert_eq!(g.content, g.bounds, "all of the source, in its box");
    assert_eq!(uv, whole);

    let (g, _) = laid_out(
        image(ImageFit::None).size(10.0, 10.0),
        SizeProposal::exact(50.0, 50.0),
    );
    assert_eq!(
        g.content,
        Rect::new(3.0, 3.0, 4.0, 4.0),
        "centred at its natural size in a larger box"
    );
    let (g, _) = laid_out(
        image(ImageFit::ScaleDown).size(10.0, 10.0),
        SizeProposal::exact(50.0, 50.0),
    );
    assert_eq!(
        g.content,
        Rect::new(3.0, 3.0, 4.0, 4.0),
        "never grown past it"
    );
    let (g, _) = laid_out(
        image(ImageFit::Contain).size(10.0, 10.0),
        SizeProposal::exact(50.0, 50.0),
    );
    assert_eq!(g.content, g.bounds, "a scaling fit is unchanged");

    // Without device pixels, the natural size is the pixel count.
    let (g, _) = laid_out(
        LiveImage::new(live(8, 8).0)
            .fit(ImageFit::None)
            .size(10.0, 10.0)
            .alt("x"),
        SizeProposal::exact(50.0, 50.0),
    );
    assert_eq!(g.content, Rect::new(1.0, 1.0, 8.0, 8.0));
}

#[test]
fn c7_a_quarter_turn_and_a_resize_reshape_the_box() {
    let (source, writer) = live(720, 1280);
    let image = LiveImage::new(source).alt("x");
    let handle = image.handle();
    let mut tree = tree();
    let id = tree.add(image);
    // `Center` proposes both axes, as a window does; an `HStack` would leave
    // the width open and the box would follow the height alone.
    tree.add(Center::new().child(id));
    let at = SizeProposal::exact(864.0, 754.0);
    tree.layout(at);
    let b = tree.bounds(id);
    assert!(close((b.width, b.height), (424.0, 754.0)));
    commit(&writer, 1280, 720, 1);
    tree.layout(at);
    let b = tree.bounds(id);
    assert!(close((b.width, b.height), (864.0, 486.0)));
    assert_eq!(handle.frame_size().get(), Some((1280, 720)));
    commit(&writer, 720, 1280, 2);
    tree.layout(at);
    let b = tree.bounds(id);
    assert!(close((b.width, b.height), (424.0, 754.0)));

    // A camera's 1280 x 720 turned a quarter is displayed 720 x 1280.
    let (camera, _w) = live(1280, 720);
    let turned = LiveImage::new(camera)
        .orientation(ImageOrientation::Rotate90)
        .alt("x");
    assert!(close(size_of(&turned, at, 1.0), (424.0, 754.0)));
}

// ── C.3-C.6: what a commit costs ──

/// A LiveImage beside a sibling that counts its paints, in a tree with a
/// counting waker, laid out, rendered and consumed once.
struct Scene {
    tree: WidgetTree,
    waker: Arc<CountingWaker>,
    mirror: LiveImageMirror,
    sibling_paints: Rc<Cell<u32>>,
    handle: LiveImageHandle,
    writer: LiveImageWriter,
}

impl Scene {
    fn new(w: u32, h: u32) -> Self {
        let (source, writer) = live(w, h);
        let waker = Arc::new(CountingWaker::new());
        let mut tree = tree();
        tree.set_redraw_waker(Some(waker.clone() as Arc<dyn RedrawWaker>));
        let image = LiveImage::new(source).size(64.0, 48.0).alt("x");
        let handle = image.handle();
        let sibling_paints = Rc::new(Cell::new(0));
        tree.add(
            HStack::new()
                .child(image)
                .child(Painter(sibling_paints.clone())),
        );
        let mut scene = Self {
            tree,
            waker,
            mirror: LiveImageMirror::new(),
            sibling_paints,
            handle,
            writer,
        };
        let first = scene.frame();
        assert_eq!(first.full_uploads, 1, "the first frame uploads the picture");
        scene
    }

    fn frame(&mut self) -> teksilo_canvas::live_image::testing::MirrorReport {
        self.tree.layout(SizeProposal::exact(400.0, 300.0));
        let frame = self.tree.render();
        self.mirror.consume(&frame)
    }

    fn paints(&self) -> (u32, u64) {
        (
            self.sibling_paints.get(),
            self.handle.stats().attachment.paints,
        )
    }
}

#[test]
fn c3_a_hundred_commits_repaint_nothing_and_upload_once() {
    let mut scene = Scene::new(32, 24);
    let before = scene.paints();
    for seed in 1..=100 {
        commit(&scene.writer, 32, 24, seed);
    }
    let report = scene.frame();
    assert_eq!(scene.paints(), before, "neither widget painted");
    assert_eq!(
        report.full_uploads + report.partial_uploads,
        1,
        "one upload"
    );
    assert_eq!(
        scene.handle.stats().attachment.window_generation,
        101,
        "of the latest commit"
    );
}

#[test]
fn c4_rect_commits_upload_their_union() {
    let mut scene = Scene::new(32, 24);
    let before = scene.paints();
    let rect = PixelRect::new(4, 3, 5, 2);
    for seed in 1..=10u8 {
        let px: Vec<u8> = [seed, 0, 0, 255].repeat(10);
        scene.writer.write_rect(rect, &px, 5 * 4).unwrap();
    }
    let report = scene.frame();
    assert_eq!(scene.paints(), before);
    assert_eq!((report.full_uploads, report.partial_uploads), (0, 1));
    assert_eq!(report.rects, vec![rect]);
    assert_eq!(report.bytes, 5 * 2 * 4);
}

#[test]
fn c5_a_burst_of_commits_wakes_the_window_once() {
    let mut scene = Scene::new(32, 24);
    let before = scene.waker.count();
    for seed in 1..=100 {
        commit(&scene.writer, 32, 24, seed);
    }
    assert_eq!(scene.waker.count(), before + 1);
    assert_eq!(scene.waker.count_of(WakeKind::Draw), 1);
    scene.frame();
}

#[test]
fn c6_an_idle_tree_replays_its_frame_and_wakes_nobody() {
    let mut scene = Scene::new(32, 24);
    let wakes = scene.waker.count();
    let first = scene.tree.render();
    let address = Rc::as_ptr(&first);
    // The cache-hit path makes the frame unique first, so the previous
    // handle has to go for the address to be comparable.
    drop(first);
    assert!(!scene.tree.needs_render());
    let again = scene.tree.render();
    assert_eq!(Rc::as_ptr(&again), address, "the same frame, replayed");
    assert_eq!(scene.waker.count(), wakes);
}

// ── C.10-C.12: placement and mapping through the real widget ──

/// The spike's portrait case, inset by 10 so a window point and a local one
/// differ: 720 x 1280 under (864, 754), `Contain`.
fn portrait_tree(
    wrap: impl FnOnce(LiveImage) -> Box<dyn Widget>,
) -> (WidgetTree, LiveImageHandle, LiveImageWriter) {
    let (source, writer) = live(720, 1280);
    let image = LiveImage::new(source).alt("screen");
    let handle = image.handle();
    let mut tree = tree();
    let inner = tree.add_boxed(wrap(image));
    let row = tree.add(HStack::new().child(inner));
    tree.add(Padding::uniform(10.0).child(row));
    tree.layout(SizeProposal::exact(884.0, 774.0));
    (tree, handle, writer)
}

#[test]
fn c10_a_press_maps_to_the_pixel_drawn_under_it_before_any_render() {
    let pressed: Rc<RefCell<Vec<Point>>> = Rc::default();
    let seen = pressed.clone();
    let (mut tree, handle, _writer) = portrait_tree(|image| {
        Box::new(image.on_pointer_event(move |event, _ctx| {
            if let WidgetEvent::PointerDown { position, .. } = event {
                seen.borrow_mut().push(*position);
            }
            EventResponse::Handled
        }))
    });
    let geometry = handle.geometry().expect("placed by the layout alone");
    assert_eq!(geometry.content, Rect::new(0.0, 0.0, 424.0, 754.0));
    for (local, expected) in [
        (Point::new(0.0, 0.0), (0, 0)),
        (Point::new(211.99, 376.99), (359, 639)),
        (Point::new(423.99, 753.99), (719, 1279)),
    ] {
        tree.pointer_down_button(
            Point::new(local.x + 10.0, local.y + 10.0),
            PointerButton::Primary,
        );
        tree.pointer_up_button(
            Point::new(local.x + 10.0, local.y + 10.0),
            PointerButton::Primary,
        );
        let at = *pressed
            .borrow()
            .last()
            .expect("the press reached the widget");
        assert_eq!(handle.map_to_source(at), Some(expected), "at {local:?}");
    }
    assert_eq!(handle.map_to_source(Point::new(424.0, 377.0)), None);
    assert_eq!(
        handle.map_to_source_clamped(Point::new(424.0, 377.0)),
        Some((719, 640))
    );
    assert_eq!(
        handle.map_to_source_clamped(Point::new(-5.0, -5.0)),
        Some((0, 0))
    );
}

#[test]
fn c11_paint_draws_the_placement_the_layout_computed() {
    for scale in [1.0, 1.25] {
        let (source, _writer) = live(720, 1280);
        let image = LiveImage::new(source).alt("x");
        let handle = image.handle();
        let mut tree = tree();
        tree.set_device_scale_factor(scale);
        let inner = tree.add(image);
        let row = tree.add(HStack::new().child(inner));
        tree.add(Padding::uniform(10.3).child(row));
        tree.layout(SizeProposal::exact(884.6, 774.6));
        let geometry = handle.geometry().expect("placed");
        let bounds = tree.bounds(inner);
        let frame = tree.render();
        let quad = only_quad(&frame);
        let visible = geometry.visible().expect("something shows");
        let expected = [
            visible.x + bounds.x,
            visible.y + bounds.y,
            visible.width,
            visible.height,
        ];
        for (a, b) in quad.screen.iter().zip(expected) {
            assert!(
                (a - b).abs() < 1e-3,
                "at {scale}: {:?} != {expected:?}",
                quad.screen
            );
        }
        // Snapped: every edge of the picture on the device grid. Where the
        // box itself is off the grid, the quad is the picture cut to the
        // box, and its texels still land on device-pixel centres.
        let c = geometry.content;
        for edge in [
            bounds.x + c.x,
            bounds.x + c.x + c.width,
            bounds.y + c.y,
            bounds.y + c.y + c.height,
        ] {
            let device = edge * scale;
            assert!(
                (device - device.round()).abs() < 1e-3,
                "edge {edge} at {scale}"
            );
        }
        assert_eq!(
            tree.live_image_geometry(handle.widget_id().unwrap()),
            Some(geometry),
            "and records it for automation"
        );
    }
}

#[test]
fn c12_a_window_position_maps_through_the_handlers_local_frame() {
    let mapped: Rc<RefCell<Vec<(Option<(u32, u32)>, Point)>>> = Rc::default();
    let seen = mapped.clone();
    let handle_slot: Rc<RefCell<Option<LiveImageHandle>>> = Rc::default();
    let inner_handle = handle_slot.clone();
    let (mut tree, handle, _writer) = portrait_tree(move |image| {
        *inner_handle.borrow_mut() = Some(image.handle());
        Box::new(image.on_scroll(move |event, ctx| {
            if let WidgetEvent::Scroll {
                window_position: Some(window),
                ..
            } = event
            {
                let local = ctx.to_local(*window).expect("inside a dispatch");
                let handle = inner_handle.borrow();
                let pixel = handle.as_ref().unwrap().map_to_source(local);
                seen.borrow_mut().push((pixel, local));
            }
            EventResponse::Handled
        }))
    });
    let window = Point::new(221.99, 386.99);
    tree.dispatch_event(WidgetEvent::scroll_at(
        ScrollDelta::Lines { x: 0.0, y: 1.0 },
        Modifiers::NONE,
        window,
    ));
    let (pixel, local) = mapped.borrow()[0];
    assert_eq!(local, Point::new(211.99, 376.99));
    assert_eq!(pixel, handle.map_to_source(Point::new(211.99, 376.99)));
    assert_eq!(pixel, Some((359, 639)));
}

// ── Snapping under ancestors' transforms ──

/// `fit(Fill)` so the picture is the box: 10.3 x 7.7 at the origin.
fn snapped_width(ancestor: Option<Transform2D>, scale: f32) -> f32 {
    let (source, _writer) = live(16, 12);
    let image = LiveImage::new(source)
        .size(10.3, 7.7)
        .fit(ImageFit::Fill)
        .alt("x");
    let handle = image.handle();
    let mut tree = tree();
    tree.set_device_scale_factor(scale);
    let child = match ancestor {
        Some(t) => tree.add(Transformed::new(t, image)),
        None => tree.add(image),
    };
    tree.add(HStack::new().child(child));
    tree.layout(SizeProposal::exact(100.0, 100.0));
    handle.geometry().unwrap().content.width
}

#[test]
fn snapping_happens_in_device_space_through_the_ancestors_transforms() {
    // 10.3 device pixels round to 10.
    assert!((snapped_width(None, 1.0) - 10.0).abs() < 1e-4);
    // Under a 2x ancestor the edge lands on device pixel 20.6, which rounds
    // to 21: 10.5 widget-local.
    assert!((snapped_width(Some(Transform2D::scale(2.0, 2.0)), 1.0) - 10.5).abs() < 1e-4);
    // The ancestor's scale and the device scale compose: 10.3 x 3 = 30.9,
    // so 31 / 3.
    assert!((snapped_width(Some(Transform2D::scale(2.0, 2.0)), 1.5) - 31.0 / 3.0).abs() < 1e-4);
    // A rotated ancestor keeps no edge on the grid: nothing snaps.
    assert!((snapped_width(Some(Transform2D::rotate(0.3)), 1.0) - 10.3).abs() < 1e-4);
}

#[test]
fn snapping_can_be_turned_off() {
    let (source, _writer) = live(16, 12);
    let image = LiveImage::new(source)
        .size(10.3, 7.7)
        .fit(ImageFit::Fill)
        .pixel_snap(false)
        .alt("x");
    let handle = image.handle();
    let mut tree = tree();
    let id = tree.add(image);
    tree.add(HStack::new().child(id));
    tree.layout(SizeProposal::exact(100.0, 100.0));
    assert!((handle.geometry().unwrap().content.width - 10.3).abs() < 1e-4);
    assert!(
        !only_quad(&tree.render()).pixel_snap,
        "nor may the renderer snap it: the quad says so"
    );
}

#[test]
fn a_snapped_picture_lets_the_renderer_snap_its_quad() {
    let (source, _writer) = live(16, 12);
    let mut tree = tree();
    let id = tree.add(LiveImage::new(source).alt("x"));
    tree.add(HStack::new().child(id));
    tree.layout(SizeProposal::exact(100.0, 100.0));
    assert!(only_quad(&tree.render()).pixel_snap);
}

#[test]
fn paint_follows_a_box_moved_without_a_layout() {
    let (source, _writer) = live(16, 12);
    let image = LiveImage::new(source).size(16.0, 12.0).alt("x");
    let handle = image.handle();
    let mut tree = tree();
    let id = tree.add(image);
    tree.add(HStack::new().child(id));
    tree.layout(SizeProposal::exact(100.0, 100.0));
    let laid_out = tree.bounds(id);
    assert_eq!((laid_out.width, laid_out.height), (16.0, 12.0));
    let widget = tree
        .widget_as_any(id)
        .and_then(|any| any.downcast_ref::<LiveImage>())
        .expect("the widget");
    let theme = teksilo_core::presets::intui::light();
    let ctx = PaintContext {
        theme: &theme,
        scale_factor: 1.0,
        text_scale: 1.0,
        layout_direction: LayoutDirection::LeftToRight,
        effective_enabled: true,
        prefers_high_contrast: false,
        prefers_reduced_motion: false,
        prefers_large_text: false,
        window_active: true,
        clip_bounds: None,
    };
    let mut canvas = Canvas::new();
    let moved = Rect::new(30.4, 20.0, 16.0, 12.0);
    widget.paint(moved, &mut canvas, &ctx);
    let frame = canvas.into_render_frame();
    let quad = only_quad(&frame);
    // The picture follows the box, its edges snapped again at the new
    // place: device pixels 30 to 46, cut to the box at 30.4.
    let content = handle.geometry().unwrap().content;
    assert!((content.x + 0.4).abs() < 1e-4 && (content.width - 16.0).abs() < 1e-4);
    let expected = [30.4, 20.0, 15.6, 12.0];
    for (a, b) in quad.screen.iter().zip(expected) {
        assert!((a - b).abs() < 1e-4, "{:?}", quad.screen);
    }
}

// ── Background, letterbox, placeholder ──

fn fills(frame: &RenderFrame, color: Color) -> Vec<[f32; 4]> {
    frame
        .decorations
        .iter()
        .filter(|d| d.color == color.to_array())
        .map(|d| d.rect)
        .collect()
}

#[test]
fn the_background_fills_the_letterbox_and_never_the_picture() {
    let red = Color::from_rgb(1.0, 0.0, 0.0);
    let (source, writer) = live(100, 50);
    let mut tree = tree();
    tree.add(
        LiveImage::new(source)
            .size(100.0, 100.0)
            .background(red)
            .alt("x"),
    );
    tree.layout(SizeProposal::exact(100.0, 100.0));
    let frame = tree.render();
    let mut bars = fills(&frame, red);
    bars.sort_by(|a, b| a[1].total_cmp(&b[1]));
    assert_eq!(
        bars,
        vec![[0.0, 0.0, 100.0, 25.0], [0.0, 75.0, 100.0, 25.0]]
    );

    // Not live: the whole box, and still the one quad.
    writer.clear().unwrap();
    tree.layout(SizeProposal::exact(100.0, 100.0));
    let frame = tree.render();
    assert_eq!(fills(&frame, red), vec![[0.0, 0.0, 100.0, 100.0]]);
    assert_eq!(only_quad(&frame).painted, None);
}

#[test]
fn the_placeholder_shows_while_the_source_is_not_live() {
    let source = LiveImageSource::new(LivePixelFormat::Rgba8);
    let writer = source.writer();
    let mut tree = tree();
    tree.add(
        LiveImage::new(source)
            .size(200.0, 100.0)
            .placeholder("Starting")
            .alt("x"),
    );
    tree.layout(SizeProposal::exact(200.0, 100.0));
    let frame = tree.render();
    assert!(!frame.glyphs.is_empty(), "the placeholder is drawn");
    only_quad(&frame);
    commit(&writer, 8, 8, 1);
    tree.layout(SizeProposal::exact(200.0, 100.0));
    let frame = tree.render();
    assert!(
        frame.glyphs.is_empty(),
        "a live picture shows no placeholder"
    );
}

// ── C.13, C.14: accessibility ──

fn image_node(
    update: &teksilo_core::accesskit::TreeUpdate,
) -> Option<&teksilo_core::accesskit::Node> {
    update
        .nodes
        .iter()
        .map(|(_, node)| node)
        .find(|node| node.role() == Role::Image)
}

#[test]
fn c13_one_image_node_named_by_alt() {
    let (source, writer) = live(16, 12);
    let mut tree = tree();
    tree.add(LiveImage::new(source).alt("Virtual machine screen"));
    tree.layout(SizeProposal::exact(100.0, 100.0));
    let update = tree.sync_accessibility();
    let node = image_node(&update).expect("a Role::Image node");
    assert_eq!(node.label(), Some("Virtual machine screen"));
    assert_eq!(node.description(), None);

    // Pixels are not accessible content: ten commits change no node.
    let _ = tree.render();
    let _ = tree.sync_accessibility();
    let version = tree.at_version().get();
    let before = tree.accessibility_tree_snapshot();
    for seed in 1..=10 {
        commit(&writer, 16, 12, seed);
        tree.layout(SizeProposal::exact(100.0, 100.0));
        let _ = tree.render();
    }
    let _ = tree.sync_accessibility();
    assert_eq!(tree.at_version().get(), version, "no content changed");
    assert_eq!(tree.accessibility_tree_snapshot().nodes, before.nodes);
}

#[test]
fn c13_a_decorative_picture_is_hidden() {
    let (source, _writer) = live(16, 12);
    let mut tree = tree();
    tree.add(LiveImage::new(source).a11y_hidden());
    tree.layout(SizeProposal::exact(100.0, 100.0));
    let update = tree.sync_accessibility();
    assert!(image_node(&update).is_none());
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "LiveImage has no alt text")]
fn c13_a_picture_with_neither_alt_nor_hidden_asserts() {
    let (source, _writer) = live(16, 12);
    let mut tree = tree();
    tree.add(LiveImage::new(source));
    tree.layout(SizeProposal::exact(100.0, 100.0));
    let _ = tree.sync_accessibility();
}

#[test]
fn c14_the_placeholder_is_the_description_while_not_live() {
    let source = LiveImageSource::new(LivePixelFormat::Rgba8);
    let writer = source.writer();
    let placeholder = Signal::new("Starting".to_string());
    let mut tree = tree();
    tree.add(
        LiveImage::new(source)
            .size(100.0, 100.0)
            .placeholder(placeholder.clone())
            .alt("Screen"),
    );
    tree.layout(SizeProposal::exact(100.0, 100.0));
    let update = tree.sync_accessibility();
    assert_eq!(image_node(&update).unwrap().description(), Some("Starting"));

    placeholder.set("Booting".to_string());
    tree.layout(SizeProposal::exact(100.0, 100.0));
    let update = tree.sync_accessibility();
    assert_eq!(image_node(&update).unwrap().description(), Some("Booting"));

    commit(&writer, 8, 8, 1);
    tree.layout(SizeProposal::exact(100.0, 100.0));
    let update = tree.sync_accessibility();
    assert_eq!(image_node(&update).unwrap().description(), None);
}

#[test]
fn a_bound_alt_reaches_the_node_when_it_changes() {
    let (source, _writer) = live(16, 12);
    let alt = Signal::new("Screen".to_string());
    let mut tree = tree();
    tree.add(LiveImage::new(source).alt(alt.clone()));
    tree.layout(SizeProposal::exact(100.0, 100.0));
    let _ = tree.sync_accessibility();
    alt.set("Écran".to_string());
    tree.layout(SizeProposal::exact(100.0, 100.0));
    let update = tree.sync_accessibility();
    assert_eq!(image_node(&update).unwrap().label(), Some("Écran"));
}

// ── C.15: release and source switches ──

#[test]
fn c15_a_destroyed_widget_leaves_no_quad_and_no_texture() {
    let (source, _writer) = live(16, 12);
    let mut tree = tree();
    let id = tree.add(LiveImage::new(source.clone()).alt("x"));
    tree.add(
        HStack::new()
            .child(id)
            .child(Painter(Rc::new(Cell::new(0)))),
    );
    let mut mirror = LiveImageMirror::new();
    tree.layout(SizeProposal::exact(100.0, 100.0));
    mirror.consume(&tree.render());
    assert_eq!(mirror.texture_count(), 1);
    tree.destroy_subtree_for_testing(id);
    tree.layout(SizeProposal::exact(100.0, 100.0));
    let frame = tree.render();
    assert!(frame.live_images.is_empty());
    mirror.consume(&frame);
    assert_eq!(mirror.texture_count(), 0);
    assert_eq!(source.stats().attachments, 0, "and its attachment detached");
}

/// Parks its one child while `park` is set: a culling container whose
/// decision no binding watches.
#[derive(Debug)]
struct Culler {
    child: Option<Box<dyn Widget>>,
    id: Option<WidgetId>,
    park: Rc<Cell<bool>>,
}

impl Widget for Culler {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.add_boxed(self.child.take().expect("built once"));
        self.id = Some(id);
        vec![id]
    }
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
    fn children(&self) -> Vec<WidgetId> {
        self.id.into_iter().collect()
    }
    fn culls_children(&self) -> bool {
        true
    }
    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for placement in children.iter_mut() {
            placement.origin = bounds.origin();
            placement.size = bounds.size();
            placement.dormant = self.park.get();
        }
    }
}

#[test]
fn c15_a_culled_picture_frees_its_texture_at_the_next_frame() {
    // The picture's own scroll handler asks to be parked, so the pass that
    // parks it marks nothing else: the frame must still be composed again,
    // or it replays the picture and its texture stays.
    let (source, _writer) = live(16, 12);
    let park = Rc::new(Cell::new(false));
    let asks = park.clone();
    let image = LiveImage::new(source.clone())
        .alt("x")
        .on_scroll(move |_event, _ctx| {
            asks.set(true);
            EventResponse::Handled
        });
    let mut tree = tree();
    tree.add(Culler {
        child: Some(Box::new(image)),
        id: None,
        park,
    });
    let mut mirror = LiveImageMirror::new();
    mirror.set_park_budget(0);
    tree.layout(SizeProposal::exact(100.0, 100.0));
    mirror.consume(&tree.render());
    assert_eq!(mirror.texture_count(), 1);

    tree.dispatch_event(WidgetEvent::scroll_at(
        ScrollDelta::Lines { x: 0.0, y: -1.0 },
        Modifiers::NONE,
        Point::new(50.0, 50.0),
    ));
    tree.layout(SizeProposal::exact(100.0, 100.0));
    let frame = tree.render();
    assert!(
        frame.live_images.is_empty(),
        "the parked picture is not drawn"
    );
    mirror.consume(&frame);
    assert_eq!(mirror.texture_count(), 0, "and its texture went with it");
}

#[test]
fn c15_switching_sources_frees_the_old_texture_and_keeps_the_handles_signals() {
    let (first, _a) = live(16, 12);
    let (second, _b) = live(32, 24);
    let shown = Signal::new(first.clone());
    let image = LiveImage::new(shown.clone()).alt("x");
    let handle = image.handle();
    let (status, frame_size) = (handle.status(), handle.frame_size());
    let mut tree = tree();
    tree.add(image);
    let mut mirror = LiveImageMirror::new();
    tree.layout(SizeProposal::exact(100.0, 100.0));
    mirror.consume(&tree.render());
    assert!(mirror.pixels(first.id()).is_some());

    shown.set(second.clone());
    tree.layout(SizeProposal::exact(100.0, 100.0));
    let frame = tree.render();
    assert!(only_quad(&frame).consumer.source().ptr_eq(&second));
    mirror.consume(&frame);
    assert_eq!(mirror.texture_count(), 1);
    assert!(
        mirror.pixels(first.id()).is_none(),
        "the old texture is gone"
    );
    assert!(mirror.pixels(second.id()).is_some());
    assert!(Signal::same(&status, &handle.status()));
    assert!(Signal::same(&frame_size, &handle.frame_size()));
    assert_eq!(frame_size.get(), Some((32, 24)));
    assert!(handle.source().unwrap().ptr_eq(&second));
    assert_eq!(first.stats().attachments, 0);
}

// ── B.9: device scale ──

#[test]
fn b9_a_new_device_scale_lays_the_picture_out_again() {
    // 5 x 2.142857: 2 at scale 1, 2.4 at 1.25, under the same logical
    // proposal, which on its own lays nothing out again.
    let (source, _writer) = live(7, 3);
    let mut tree = tree();
    let id = tree.add(LiveImage::new(source).alt("x"));
    tree.add(Center::new().child(id));
    let at = SizeProposal::exact(5.0, 100.0);
    tree.layout(at);
    assert!((tree.bounds(id).height - 2.0).abs() < 1e-4);
    tree.set_device_scale_factor(1.25);
    tree.layout(at);
    assert!((tree.bounds(id).height - 2.4).abs() < 1e-4);
}

// ── The handle and the Debug repr ──

#[test]
fn a_handle_made_first_follows_the_widget_that_takes_it() {
    let handle = LiveImageHandle::new();
    assert!(handle.source().is_none());
    assert_eq!(handle.frame_size().get(), None);
    assert_eq!(handle.status().get(), LiveImageStatus::Disconnected);
    let (source, _writer) = live(16, 12);
    let image = LiveImage::new(source.clone()).with_handle(&handle).alt("x");
    assert_eq!(handle.frame_size().get(), Some((16, 12)), "before mount");
    assert_eq!(handle.status().get(), LiveImageStatus::Live);
    assert!(handle.source().unwrap().ptr_eq(&source));
    assert!(handle.widget_id().is_none());

    let mut tree = tree();
    let id = tree.add(image);
    tree.layout(SizeProposal::exact(100.0, 100.0));
    assert_eq!(handle.widget_id(), Some(id));
    assert!(handle.geometry().is_some());
    let _ = tree.render();
    assert_eq!(handle.stats().attachment.paints, 1);

    // Gone with the widget.
    tree.destroy_subtree_for_testing(id);
    assert!(handle.widget_id().is_none());
    assert!(handle.geometry().is_none());
    assert_eq!(handle.stats().attachment.paints, 0);
    assert_eq!(handle.stats().source.generation, 1);
}

#[test]
fn a_picture_with_no_frame_size_has_no_placement() {
    let source = LiveImageSource::new(LivePixelFormat::Rgba8);
    let writer = source.writer();
    let image = LiveImage::new(source).alt("x");
    let handle = image.handle();
    let mut tree = tree();
    let id = tree.add(image);
    tree.layout(SizeProposal::exact(100.0, 100.0));
    assert!(handle.geometry().is_none());
    assert!(tree.live_image_geometry(id).is_none());
    assert_eq!(handle.map_to_source(Point::new(1.0, 1.0)), None);
    commit(&writer, 16, 12, 1);
    tree.layout(SizeProposal::exact(100.0, 100.0));
    assert!(handle.geometry().is_some());
    assert!(tree.live_image_geometry(id).is_some());
    // A cleared buffer keeps its size as the hint: the box and the
    // placement stay, and the placeholder shows in them.
    writer.clear().unwrap();
    tree.layout(SizeProposal::exact(100.0, 100.0));
    assert_eq!(handle.status().get(), LiveImageStatus::Waiting);
    assert!(handle.geometry().is_some());
}

#[test]
fn the_debug_repr_reads_counters_not_the_lock() {
    let source = LiveImageSource::builder(LivePixelFormat::Bgrx8)
        .label("vm-screen")
        .build();
    let writer = source.writer();
    writer
        .write_frame(720, 1280, &vec![0; 720 * 1280 * 4], 720 * 4)
        .unwrap();
    let image = LiveImage::new(source).alt("x");
    let before = format!("{image:?}");
    assert!(before.contains(&format!("{}", image.handle().source().unwrap().id())));
    assert!(
        before.contains("\"vm-screen\" Bgrx8 720x1280 Live"),
        "{before}"
    );
    assert!(before.contains("window_gen: 0"), "{before}");
    assert!(before.contains("content: none"), "{before}");

    let mut tree = tree();
    let id = tree.add(image);
    tree.add(HStack::new().child(id));
    tree.layout(SizeProposal::exact(864.0, 754.0));
    // The window's live pass is what moves `window_gen`.
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&tree.render());
    // A transaction held on this thread: the repr still reads.
    let guard = writer.lock().unwrap();
    let repr = tree.widget_debug_string(id).expect("a repr");
    drop(guard);
    assert!(repr.contains("gen: 1"), "{repr}");
    assert!(repr.contains("window_gen: 1"), "{repr}");
    assert!(repr.contains("sizing: Aspect"), "{repr}");
    assert!(repr.contains("fit: Contain"), "{repr}");
    assert!(repr.contains("orientation: Normal"), "{repr}");
    assert!(repr.contains("scaling: Linear"), "{repr}");
    assert!(repr.contains("content: (0, 0, 424, 754)"), "{repr}");
    assert!(repr.contains("paints: 1"), "{repr}");
}
