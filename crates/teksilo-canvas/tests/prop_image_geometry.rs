// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Property tests for `ImageGeometry` (crates/teksilo-canvas/src/image_geometry.rs):
//! the placement a widget paints a raster from and maps pointer input
//! through. A pixel that maps back to another one is a press the guest of a
//! VM screen receives a pixel off; an unstable snap is a picture that
//! shimmers between layouts.
//!
//! The unit tests in `image_geometry/tests.rs` pin exact values (the spec's
//! worked mappings, the agreement with `apply_orientation`, one sweep over a
//! fixed grid of fits, orientations and scales). This file generalises the
//! relations behind them over arbitrary boxes, sources and device grids.
//! Manual override knob: `PROPTEST_CASES=N cargo test -p teksilo-canvas
//! --test prop_image_geometry`.

use proptest::prelude::*;
use teksilo_canvas::image_geometry::oriented_crop;
use teksilo_canvas::{
    ImageFit, ImageGeometry, ImageOrientation, PixelRect, Point, Rect, Size, Transform2D,
};
use teksilo_tokens::{Alignment, HAlignment, VAlignment};

fn arb_orientation() -> impl Strategy<Value = ImageOrientation> {
    (1u16..=8).prop_map(ImageOrientation::from_tiff)
}

fn arb_fit() -> impl Strategy<Value = ImageFit> {
    prop_oneof![
        Just(ImageFit::Contain),
        Just(ImageFit::Cover),
        Just(ImageFit::Fill),
        Just(ImageFit::ScaleDown),
        Just(ImageFit::None),
    ]
}

fn arb_alignment() -> impl Strategy<Value = Alignment> {
    let h = prop_oneof![
        Just(HAlignment::Leading),
        Just(HAlignment::Center),
        Just(HAlignment::Trailing)
    ];
    let v = prop_oneof![
        Just(VAlignment::Top),
        Just(VAlignment::Center),
        Just(VAlignment::Bottom)
    ];
    (h, v).prop_map(|(horizontal, vertical)| Alignment {
        horizontal,
        vertical,
    })
}

/// A device grid: the widget's window origin, then an axis-aligned ancestor
/// scale (possibly mirrored), then a HiDPI scale. Cost: constant per case.
fn arb_to_device() -> impl Strategy<Value = Transform2D> {
    (
        -500.0f32..500.0,
        -500.0f32..500.0,
        prop_oneof![Just(1.0f32), Just(2.0), Just(0.5), Just(-1.0), 0.25f32..4.0],
        prop_oneof![Just(1.0f32), Just(1.25), Just(1.5), Just(2.0), 0.75f32..3.0],
    )
        .prop_map(|(ox, oy, ancestor, hidpi)| {
            Transform2D::translate(ox, oy)
                .then(&Transform2D::scale(ancestor, ancestor))
                .then(&Transform2D::scale(hidpi, hidpi))
        })
}

/// A placement and a source pixel inside it. Boxes up to 400 dp and sources
/// up to 64 px a side keep every case to one `compute` and a few float
/// operations; the pixel is drawn from the source's own size
/// (`prop_flat_map`), never independently of it.
fn arb_geometry_and_pixel() -> impl Strategy<Value = (ImageGeometry, (u32, u32), bool)> {
    (
        (1.0f32..400.0, 1.0f32..400.0),
        (1u32..=64, 1u32..=64),
        arb_fit(),
        arb_alignment(),
        any::<bool>(),
        arb_orientation(),
        prop::option::of(arb_to_device()),
    )
        .prop_flat_map(|((bw, bh), (sw, sh), fit, alignment, rtl, o, device)| {
            let g = ImageGeometry::compute(Size::new(bw, bh), (sw, sh), fit, alignment, rtl, o);
            let g = match device {
                Some(t) => g.snapped(t),
                None => g,
            };
            (Just(g), (0..sw, 0..sh), Just(device.is_some()))
        })
}

fn inside(r: Rect, p: Point) -> bool {
    p.x >= r.x && p.x < r.x + r.width && p.y >= r.y && p.y < r.y + r.height
}

// ── 1. The centre of where a source pixel is displayed maps back to that
//      pixel, whenever that centre is inside the box: drawing and input
//      agree for every fit, alignment, direction, orientation and grid ──
proptest! {
    #![proptest_config(ProptestConfig { cases: 1024, ..ProptestConfig::default() })]
    #[test]
    fn a_pixels_displayed_centre_maps_back_to_that_pixel(
        (g, (x, y), _snapped) in arb_geometry_and_pixel()
    ) {
        let r = g.map_from_source(PixelRect::new(x, y, 1, 1));
        prop_assert!(r.is_some(), "pixel ({}, {}) of {:?} has no displayed square", x, y, g);
        let r = r.unwrap();
        let centre = Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0);
        let mapped = g.map_to_source(centre);
        if inside(g.bounds, centre) {
            prop_assert_eq!(
                mapped, Some((x, y)),
                "centre {:?} of pixel ({}, {}) in {:?}", centre, x, y, g
            );
        } else {
            prop_assert_eq!(mapped, None, "centre {:?} outside the box of {:?}", centre, g);
        }
    }
}

// ── 2. Snapping is idempotent, lands every edge of a non-collapsed axis on
//      the device grid, and moves no edge by more than half a device pixel ──
proptest! {
    #![proptest_config(ProptestConfig { cases: 1024, ..ProptestConfig::default() })]
    #[test]
    fn snapping_is_idempotent_and_lands_edges_on_the_grid(
        content in (-300.0f32..300.0, -300.0f32..300.0, 0.0f32..400.0, 0.0f32..400.0),
        to_device in arb_to_device(),
    ) {
        let (x, y, w, h) = content;
        let g = ImageGeometry::new(
            (16, 16),
            ImageOrientation::Normal,
            Rect::new(x, y, w, h),
            Rect::new(0.0, 0.0, 400.0, 400.0),
        );
        let once = g.snapped(to_device);
        let twice = once.snapped(to_device);
        prop_assert_eq!(once, twice, "snapping twice moved {:?} under {:?}", g, to_device);
        let [a, _, _, d, tx, ty] = to_device.m;
        let c = once.content;
        let original = [x, x + w, y, y + h];
        let checks = [
            (c.x, a, tx, original[0], c.width != w || c.x != x),
            (c.x + c.width, a, tx, original[1], c.width != w || c.x != x),
            (c.y, d, ty, original[2], c.height != h || c.y != y),
            (c.y + c.height, d, ty, original[3], c.height != h || c.y != y),
        ];
        for (edge, scale, offset, before, axis_snapped) in checks {
            let device = edge * scale + offset;
            let moved = ((edge - before) * scale).abs();
            prop_assert!(
                moved <= 0.5 + 1e-2,
                "edge {} moved {} device px from {} under {:?}", edge, moved, before, to_device
            );
            if axis_snapped {
                prop_assert!(
                    (device - device.round()).abs() < 2e-2,
                    "edge {} sits at device {} under {:?}", edge, device, to_device
                );
            }
        }
    }
}

// ── 3. Inside the picture and the box, the clamped mapping is the exact
//      one, and the continuous mapping floors to it ──
proptest! {
    #[test]
    fn inside_the_picture_the_three_mappings_agree(
        (g, _, _) in arb_geometry_and_pixel(),
        (fx, fy) in (0.0f32..1.0, 0.0f32..1.0),
    ) {
        let Some(visible) = g.visible() else { return Ok(()) };
        let p = Point::new(visible.x + fx * visible.width, visible.y + fy * visible.height);
        let exact = g.map_to_source(p);
        prop_assume!(exact.is_some());
        prop_assert_eq!(g.map_to_source_clamped(p), exact, "at {:?} in {:?}", p, g);
        let (sx, sy) = g.map_to_source_f32(p).unwrap();
        let (ex, ey) = exact.unwrap();
        // Within float error of a pixel boundary, floor may land either side.
        prop_assert!(
            (sx - ex as f32 - 0.5).abs() <= 0.5 + 1e-3 && (sy - ey as f32 - 0.5).abs() <= 0.5 + 1e-3,
            "continuous ({}, {}) is outside pixel ({}, {}) at {:?} in {:?}", sx, sy, ex, ey, p, g
        );
    }
}

// ── 4. A crop's corners sample the texture where the mapping says those
//      points lie, so a drawn quad shows exactly the pixels input maps to ──
proptest! {
    #[test]
    fn crop_corners_sample_the_mapped_points((g, _, _) in arb_geometry_and_pixel()) {
        let Some((rect, uv)) = oriented_crop(g.content, g.bounds, g.orientation) else {
            prop_assert!(g.visible().is_none(), "no crop, yet {:?} shows something", g);
            return Ok(());
        };
        prop_assert_eq!(Some(rect), g.visible(), "crop of {:?}", g);
        let corners = [
            Point::new(rect.x, rect.y),
            Point::new(rect.x + rect.width, rect.y),
            Point::new(rect.x + rect.width, rect.y + rect.height),
            Point::new(rect.x, rect.y + rect.height),
        ];
        let (w, h) = g.source;
        for (corner, [u, v]) in corners.into_iter().zip(uv) {
            let (sx, sy) = g.map_to_source_f32(corner).unwrap();
            prop_assert!(
                (u - sx / w as f32).abs() < 1e-3 && (v - sy / h as f32).abs() < 1e-3,
                "corner {:?}: uv ({}, {}) against source ({}, {}) in {:?}", corner, u, v, sx, sy, g
            );
        }
    }
}

// ── 5. `union` holds both rects and nothing beyond their bounding box;
//      `intersect` is the overlap both hold ──
proptest! {
    #[test]
    fn pixel_rect_union_and_intersect_bound_their_operands(
        a in (0u32..1000, 0u32..1000, 0u32..500, 0u32..500),
        b in (0u32..1000, 0u32..1000, 0u32..500, 0u32..500),
    ) {
        let a = PixelRect::new(a.0, a.1, a.2, a.3);
        let b = PixelRect::new(b.0, b.1, b.2, b.3);
        let u = a.union(&b);
        for r in [a, b] {
            if !r.is_empty() {
                prop_assert_eq!(u.intersect(&r), Some(r), "{:?} ∪ {:?} = {:?} misses {:?}", a, b, u, r);
            }
        }
        if !a.is_empty() && !b.is_empty() {
            prop_assert_eq!(u.x, a.x.min(b.x), "{:?} ∪ {:?}", a, b);
            prop_assert_eq!(
                u.x + u.width,
                (a.x + a.width).max(b.x + b.width),
                "{:?} ∪ {:?}", a, b
            );
        }
        match a.intersect(&b) {
            Some(i) => {
                prop_assert!(!i.is_empty());
                prop_assert_eq!(a.intersect(&i), Some(i), "{:?} ∩ {:?} = {:?}", a, b, i);
                prop_assert_eq!(b.intersect(&i), Some(i), "{:?} ∩ {:?} = {:?}", a, b, i);
                prop_assert!(i.area() <= a.area().min(b.area()));
            }
            None => {
                let disjoint = a.is_empty()
                    || b.is_empty()
                    || a.x + a.width <= b.x
                    || b.x + b.width <= a.x
                    || a.y + a.height <= b.y
                    || b.y + b.height <= a.y;
                prop_assert!(disjoint, "{:?} and {:?} overlap, yet intersect is None", a, b);
            }
        }
    }
}
