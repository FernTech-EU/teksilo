// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `ImageWidget::from_raw`, masked images and `Avatar` on one-commit live
//! sources (spec 7.14, J.6-J.8, J.10): a raw image draws as one live quad
//! and carries no pixels in the frame, its texture goes with the first
//! frame that does not draw it, an avatar whose image changes a hundred
//! times holds one texture, a rebuild that keeps the image keeps the
//! texture, and input the source refuses draws nothing. On the renderer's
//! live pass with textures in memory.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::live_image::testing::LiveImageMirror;
use teksilo_canvas::{
    Canvas, MockTextBackend, RasterIcon, Rect, RenderFrame, Size, SizeProposal, TextBackend,
};
use teksilo_core::WidgetId;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_widgets::avatar::Avatar;
use teksilo_widgets::primitives::image_mask::ImageMaskShape;
use teksilo_widgets::primitives::{HStack, ImageWidget, Switcher};

fn tree() -> WidgetTree {
    let backend: Rc<RefCell<dyn TextBackend>> = Rc::new(RefCell::new(MockTextBackend::new()));
    WidgetTree::new()
        .with_theme(teksilo_core::presets::intui::light())
        .with_text_backend(backend)
}

fn solid(side: u32, v: u8) -> Vec<u8> {
    [v, v, v, 255].repeat((side * side) as usize)
}

fn render(tree: &mut WidgetTree) -> Rc<RenderFrame> {
    tree.layout(SizeProposal::exact(200.0, 100.0));
    tree.render()
}

/// A 50 x 50 box that clips its one child and places it `offset` to the
/// right: past 50, the child is out of view and is not painted.
#[derive(Debug)]
struct Viewport {
    child: Option<Box<dyn Widget>>,
    id: Option<WidgetId>,
    offset: Signal<f32>,
}

impl Widget for Viewport {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.add_boxed(self.child.take().expect("built once"));
        self.id = Some(id);
        self.offset.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );
        vec![id]
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(50.0, 50.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = teksilo_canvas::Point::new(bounds.x + self.offset.get(), bounds.y);
            child.size = Size::new(16.0, 16.0);
        }
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut Canvas, _ctx: &PaintContext) {}

    fn clips_children(&self) -> bool {
        true
    }

    fn children(&self) -> Vec<WidgetId> {
        self.id.into_iter().collect()
    }
}

// ── J.6 ──

#[test]
fn j6_a_raw_image_is_one_live_quad_and_no_pixels_in_the_frame() {
    let mut tree = tree();
    tree.add(ImageWidget::from_raw(solid(12, 200), 12, 12).alt("x"));
    let frame = render(&mut tree);
    assert_eq!(frame.live_images.len(), 1);
    assert_eq!(frame.live_images[0].painted, Some((12, 12)));
    assert!(frame.images.is_empty(), "nothing on the shared path");
    assert!(
        frame.pending_images.is_empty(),
        "and no pixels to upload in it"
    );
    let mut mirror = LiveImageMirror::new();
    assert_eq!(mirror.consume(&frame).full_uploads, 1);
    let id = frame.live_images[0].consumer.source().id();
    assert_eq!(mirror.pixels(id).unwrap().2, solid(12, 200).as_slice());
}

#[test]
fn j6_a_masked_icon_draws_its_own_masked_pixels() {
    let icon = RasterIcon::from_raw(solid(8, 255), 8, 8);
    let mut tree = tree();
    tree.add(
        ImageWidget::new(&icon)
            .mask(ImageMaskShape::Circle)
            .alt("x"),
    );
    let frame = render(&mut tree);
    assert!(frame.images.is_empty() && frame.live_images.len() == 1);
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame);
    let id = frame.live_images[0].consumer.source().id();
    let (_, _, px) = mirror.pixels(id).unwrap();
    assert_eq!(px[3], 0, "a masked corner");
    assert_eq!(px[(4 * 8 + 4) * 4 + 3], 255, "the opaque centre");
}

// ── J.7 ──

#[test]
fn j7_a_destroyed_image_leaves_no_texture() {
    let mut tree = tree();
    let image = tree.add(ImageWidget::from_raw(solid(8, 1), 8, 8).alt("x"));
    let other = tree.add(ImageWidget::from_raw(solid(8, 2), 8, 8).alt("y"));
    tree.add(HStack::new().child(image).child(other));
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&render(&mut tree));
    assert_eq!(mirror.texture_count(), 2);
    tree.destroy_subtree_for_testing(image);
    mirror.consume(&render(&mut tree));
    assert_eq!(mirror.texture_count(), 1, "only the one still drawn");
}

#[test]
fn j7_a_culled_image_frees_its_texture_and_comes_back() {
    for park in [true, false] {
        let offset = Signal::new(0.0f32);
        let mut tree = tree();
        tree.add(Viewport {
            child: Some(Box::new(
                ImageWidget::from_raw(solid(8, 3), 8, 8)
                    .size(16.0, 16.0)
                    .alt("x"),
            )),
            id: None,
            offset: offset.clone(),
        });
        let mut mirror = LiveImageMirror::new();
        if !park {
            mirror.set_park_budget(0);
        }
        assert_eq!(mirror.consume(&render(&mut tree)).full_uploads, 1);
        offset.set(200.0);
        let frame = render(&mut tree);
        assert!(frame.live_images.is_empty(), "out of view, not drawn");
        mirror.consume(&frame);
        assert_eq!(mirror.texture_count(), 0, "its texture went");
        offset.set(0.0);
        let report = mirror.consume(&render(&mut tree));
        if park {
            // A small one-commit picture parks: it comes back with no
            // upload.
            assert_eq!(report.full_uploads, 0);
            assert_eq!(mirror.stats().textures_parked, 0);
        } else {
            assert_eq!(report.full_uploads, 1, "back with one upload");
        }
        assert_eq!(mirror.texture_count(), 1);
    }
}

#[test]
fn j7_an_image_on_a_dormant_page_frees_its_texture() {
    let page = Signal::new(0usize);
    let mut tree = tree();
    tree.add(
        Switcher::new(page.clone())
            .child(ImageWidget::from_raw(solid(8, 4), 8, 8).alt("x"))
            .child(ImageWidget::from_raw(solid(8, 5), 8, 8).alt("y")),
    );
    let mut mirror = LiveImageMirror::new();
    mirror.set_park_budget(0);
    mirror.consume(&render(&mut tree));
    assert_eq!(mirror.texture_count(), 1);
    page.set(1);
    let report = mirror.consume(&render(&mut tree));
    assert_eq!((mirror.texture_count(), report.full_uploads), (1, 1));
    page.set(0);
    let report = mirror.consume(&render(&mut tree));
    assert_eq!((mirror.texture_count(), report.full_uploads), (1, 1));
}

// ── J.8 and the avatar's cache ──

#[test]
fn j8_an_avatar_whose_image_changes_a_hundred_times_holds_one_texture() {
    let image: Signal<Option<Rc<RasterIcon>>> = Signal::new(None);
    let mut tree = tree();
    tree.add(
        Avatar::with_initials(lit!("JD"))
            .alt(lit!("Jane"))
            .image_signal(image.clone()),
    );
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&render(&mut tree));
    for v in 0..100u8 {
        image.set(Some(Rc::new(RasterIcon::from_raw(solid(16, v), 16, 16))));
        mirror.consume(&render(&mut tree));
        assert!(mirror.texture_count() <= 1, "at change {v}");
    }
    // The pictures it showed before are gone with their sources: none
    // waits in the parked pool either.
    mirror.consume(&render(&mut tree));
    assert_eq!(mirror.texture_count(), 1);
    assert_eq!(mirror.stats().textures_parked, 0);
}

#[test]
fn a_rebuild_that_keeps_the_avatars_image_keeps_its_texture() {
    let name = Signal::new("Jane Doe".to_string());
    let icon = RasterIcon::from_raw(solid(16, 9), 16, 16);
    let mut tree = tree();
    tree.add(
        Avatar::with_image(&icon)
            .alt(lit!("Jane"))
            .name_signal(name.clone()),
    );
    let mut mirror = LiveImageMirror::new();
    let frame = render(&mut tree);
    let shown = frame.live_images[0].consumer.source().clone();
    assert_eq!(mirror.consume(&frame).full_uploads, 1);
    drop(frame);
    name.set("Jane Roe".to_string());
    let frame = render(&mut tree);
    assert!(
        frame.live_images[0].consumer.source().ptr_eq(&shown),
        "the same source across the rebuild"
    );
    let report = mirror.consume(&frame);
    assert_eq!((report.full_uploads, report.partial_uploads), (0, 0));
}

// ── J.10 ──

#[test]
fn j10_input_the_source_refuses_draws_nothing() {
    let too_wide = teksilo_canvas::live_image::LiveImageSource::MAX_DIMENSION + 1;
    for (pixels, w, h, maskable) in [
        (Vec::new(), 0, 0, true),
        (vec![0; 7], 2, 2, true),
        // Masked, this one is cropped to a 1 x 1 square, which is valid.
        (vec![0; (too_wide * 4) as usize], too_wide, 1, false),
    ] {
        let mut tree = tree();
        let row = HStack::new().child(ImageWidget::from_raw(pixels.clone(), w, h).alt("x"));
        let row = if maskable {
            row.child(
                ImageWidget::from_raw(pixels, w, h)
                    .mask(ImageMaskShape::Circle)
                    .alt("x"),
            )
        } else {
            row
        };
        tree.add(row);
        let frame = render(&mut tree);
        assert!(frame.live_images.is_empty(), "{w}x{h} draws nothing");
        assert!(frame.images.is_empty());
    }
}

#[test]
fn an_avatar_whose_image_is_refused_shows_its_initials() {
    let mut tree = tree();
    tree.add(
        Avatar::from_raw_image(vec![0; 7], 2, 2)
            .alt(lit!("Jane"))
            .fallback_initials(lit!("JD")),
    );
    let frame = render(&mut tree);
    assert!(frame.live_images.is_empty());
    assert!(!frame.glyphs.is_empty(), "the initials");
    let update = tree.sync_accessibility();
    assert!(
        !update
            .nodes
            .iter()
            .any(|(_, n)| n.role() == teksilo_core::accesskit::Role::Image),
        "nor is it announced as an image"
    );
}
