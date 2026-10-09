// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What repainting a static image costs once its texture is queued.
//!
//! A widget showing a raster queues the image's pixels on every paint, and
//! every frame assembled from cached paint output queues them again; the
//! renderer uploads a name once and ignores it after that. Both steps used to
//! copy the whole pixel buffer, so a window that repainted continuously (a
//! level meter beside a grid of album covers) copied every visible image on
//! every frame.
//!
//! These tests count allocations at least as large as one image's pixels,
//! made on the test's own thread while the tree repaints. A copy of the
//! pixels is such an allocation; nothing else a repaint of these trees does
//! comes close to that size.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

use teksilo_canvas::{ImagePixels, RasterIcon, RenderFrame, SizeProposal};
use teksilo_core::widget::Widget;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_widgets::primitives::icon_widget::{IconMode, IconWidget};
use teksilo_widgets::primitives::{ImageWidget, VStack};

/// Side of the test image, in pixels: an album cover in a grid.
const SIDE: u32 = 304;

/// Bytes in one copy of the test image's RGBA pixels.
const PIXEL_BYTES: usize = (SIDE * SIDE * 4) as usize;

/// How many repaints each test runs after the first paint.
const REPAINTS: usize = 100;

/// The system allocator, counting what each thread asks it for.
struct CountingAllocator;

thread_local! {
    /// Allocations of at least [`PIXEL_BYTES`] this thread has made so far.
    /// A `const` thread-local of a type with no destructor never allocates,
    /// so the allocator may use it.
    static PIXEL_SIZED: Cell<u64> = const { Cell::new(0) };
}

fn count(bytes: usize) {
    if bytes >= PIXEL_BYTES {
        // `try_with` because a thread being torn down may still allocate.
        let _ = PIXEL_SIZED.try_with(|total| total.set(total.get() + 1));
    }
}

// SAFETY: every method forwards its arguments unchanged to `System`, which
// upholds the `GlobalAlloc` contract; counting touches no allocator state.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        // SAFETY: the caller's contract is `System::alloc`'s.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        // SAFETY: the caller's contract is `System::alloc_zeroed`'s.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count(new_size);
        // SAFETY: the caller's contract is `System::realloc`'s.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract is `System::dealloc`'s.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// Pixel-sized allocations made on this thread while `work` runs.
fn pixel_copies_during(work: impl FnOnce()) -> u64 {
    let before = PIXEL_SIZED.with(Cell::get);
    work();
    PIXEL_SIZED.with(Cell::get) - before
}

fn cover() -> RasterIcon {
    RasterIcon::from_raw(vec![200; PIXEL_BYTES], SIDE, SIDE)
}

/// A tree whose root holds `image` alone, laid out and painted once, so the
/// image's texture is already queued.
fn painted_once(image: impl Widget + 'static) -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new();
    let root = tree.add(VStack::new().child(image));
    tree.layout(SizeProposal::exact(400.0, 400.0));
    let frame = tree.render();
    assert_eq!(
        frame.pending_images.len(),
        1,
        "the first paint must queue the image, or its repaints measure nothing"
    );
    (tree, root)
}

/// Asserts that neither way a frame is repainted copies the image's pixels:
/// the image painting again, and the image's cached paint being reassembled
/// into a frame that something else repainted.
fn assert_repaints_copy_no_pixels(what: &str, image: impl Widget + 'static) {
    let (mut tree, root) = painted_once(image);

    let painting_again = pixel_copies_during(|| {
        for _ in 0..REPAINTS {
            tree.mark_all_needs_paint_only();
            let _ = tree.render();
        }
    });
    assert_eq!(
        painting_again, 0,
        "{what}: {painting_again} pixel-sized allocations over {REPAINTS} repaints of the image"
    );

    let reassembling = pixel_copies_during(|| {
        for _ in 0..REPAINTS {
            // The root paints nothing, so only the frame is rebuilt; the
            // image's own paint is reused from its cache.
            tree.mark_needs_paint(root);
            let _ = tree.render();
        }
    });
    assert_eq!(
        reassembling, 0,
        "{what}: {reassembling} pixel-sized allocations over {REPAINTS} frames reusing its paint"
    );
}

#[test]
fn repainting_an_image_widget_copies_no_pixels() {
    let icon = cover();
    assert_repaints_copy_no_pixels(
        "ImageWidget::new",
        ImageWidget::new(&icon).alt("Album cover"),
    );
}

#[test]
fn repainting_a_raster_icon_copies_no_pixels() {
    let icon = cover();
    assert_repaints_copy_no_pixels(
        "IconWidget::from_raster, tintable",
        IconWidget::from_raster(&icon, 24.0),
    );
    assert_repaints_copy_no_pixels(
        "IconWidget::from_raster, full colour",
        IconWidget::from_raster(&icon, 24.0).mode(IconMode::FullColor),
    );
}

/// Whether the one image `frame` queues is `icon`'s own buffer.
fn queues_the_icons_own_pixels(frame: &RenderFrame, icon: &RasterIcon) -> bool {
    matches!(
        frame.pending_images.as_slice(),
        [pending] if matches!(&pending.pixels, ImagePixels::Shared(p) if Arc::ptr_eq(p, icon.shared_pixels()))
    )
}

#[test]
fn the_queued_pixels_are_the_icons_own_buffer() {
    let icon = cover();
    for (what, image) in [
        (
            "ImageWidget::new",
            Box::new(ImageWidget::new(&icon).alt("Album cover")) as Box<dyn Widget>,
        ),
        (
            "IconWidget::from_raster, full colour",
            Box::new(IconWidget::from_raster(&icon, 24.0).mode(IconMode::FullColor)),
        ),
    ] {
        let mut tree = WidgetTree::new();
        let root = tree.add(VStack::new().child(image));
        tree.layout(SizeProposal::exact(400.0, 400.0));
        assert!(
            queues_the_icons_own_pixels(&tree.render(), &icon),
            "{what}: the first paint must queue the icon's buffer"
        );

        // A frame assembled from the cached paint goes through
        // `RenderFrame::merge`, which must not swap the buffer for a copy.
        tree.mark_needs_paint(root);
        assert!(
            queues_the_icons_own_pixels(&tree.render(), &icon),
            "{what}: a frame reusing the cached paint must still queue the icon's buffer"
        );
    }
}
