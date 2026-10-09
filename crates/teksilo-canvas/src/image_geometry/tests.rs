// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use teksilo_tokens::{Alignment, HAlignment, VAlignment};

use super::*;
use crate::exif::apply_orientation;

const ALL: [ImageOrientation; 8] = [
    ImageOrientation::Normal,
    ImageOrientation::FlipHorizontal,
    ImageOrientation::Rotate180,
    ImageOrientation::FlipVertical,
    ImageOrientation::Transpose,
    ImageOrientation::Rotate90,
    ImageOrientation::Transverse,
    ImageOrientation::Rotate270,
];

const FITS: [ImageFit; 5] = [
    ImageFit::Contain,
    ImageFit::Cover,
    ImageFit::Fill,
    ImageFit::ScaleDown,
    ImageFit::None,
];

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

/// A `w × h` RGBA image whose pixel `(x, y)` holds `x` and `y` in its first
/// two bytes, so a pixel moved by an orientation still says where it came
/// from.
fn coordinate_image(w: u32, h: u32) -> Vec<u8> {
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            px.extend_from_slice(&[x as u8, y as u8, 0, 255]);
        }
    }
    px
}

#[test]
fn from_exif_names_the_eight_values_and_nothing_else() {
    for (value, expected) in (1..=8u16).zip(ALL) {
        assert_eq!(ImageOrientation::from_exif(value), Some(expected));
        assert_eq!(ImageOrientation::from_tiff(value), expected);
    }
    for bad in [0u16, 9, 255, u16::MAX] {
        assert_eq!(ImageOrientation::from_exif(bad), None, "value {bad}");
        assert_eq!(
            ImageOrientation::from_tiff(bad),
            ImageOrientation::Normal,
            "value {bad}"
        );
    }
}

#[test]
fn quarter_turns_swap_the_displayed_size() {
    for o in ALL {
        let swapped = matches!(
            o,
            ImageOrientation::Transpose
                | ImageOrientation::Rotate90
                | ImageOrientation::Transverse
                | ImageOrientation::Rotate270
        );
        assert_eq!(o.swaps_axes(), swapped, "{o:?}");
        assert_eq!(
            o.displayed_size(3, 5),
            if swapped { (5, 3) } else { (3, 5) },
            "{o:?}"
        );
        assert_eq!(o.is_identity(), o == ImageOrientation::Normal);
    }
}

/// The mapping tables agree with `apply_orientation`, which turns real
/// pixel buffers for decoded photographs: the source pixel the table names
/// for a displayed pixel is the one `apply_orientation` moved there.
#[test]
fn the_tables_agree_with_apply_orientation() {
    let (w, h) = (3u32, 5u32);
    for o in ALL {
        let (shown, dw, dh) = apply_orientation(coordinate_image(w, h), w, h, o);
        assert_eq!((dw, dh), o.displayed_size(w, h), "{o:?}");
        for yd in 0..dh {
            for xd in 0..dw {
                let at = ((yd * dw + xd) * 4) as usize;
                let from = (u32::from(shown[at]), u32::from(shown[at + 1]));
                assert_eq!(
                    o.source_pixel(xd, yd, w, h),
                    from,
                    "{o:?} displayed ({xd}, {yd})"
                );
                assert_eq!(
                    o.displayed_pixel(from.0, from.1, w, h),
                    (xd, yd),
                    "{o:?} source {from:?}"
                );
            }
        }
    }
}

#[test]
fn the_continuous_table_is_the_pixel_table_at_pixel_centres() {
    let (w, h) = (4u32, 7u32);
    for o in ALL {
        let (dw, dh) = o.displayed_size(w, h);
        for yd in 0..dh {
            for xd in 0..dw {
                let (sx, sy) = o.source_point(xd as f32 + 0.5, yd as f32 + 0.5, w as f32, h as f32);
                let (px, py) = o.source_pixel(xd, yd, w, h);
                assert!(
                    approx(sx, px as f32 + 0.5) && approx(sy, py as f32 + 0.5),
                    "{o:?} ({xd}, {yd}): ({sx}, {sy}) against pixel ({px}, {py})"
                );
            }
        }
    }
}

#[test]
fn contain_letterboxes_and_centres() {
    let r = ImageFit::Contain.fitted_rect(
        Size::new(10.0, 10.0),
        Rect::new(0.0, 0.0, 200.0, 100.0),
        Alignment::CENTER,
        false,
    );
    assert_eq!(r, Rect::new(50.0, 0.0, 100.0, 100.0));
}

#[test]
fn each_fit_sizes_as_css_object_fit_does() {
    let bounds = Rect::new(10.0, 20.0, 200.0, 100.0);
    let small = Size::new(40.0, 10.0); // 4:1, smaller than the box
    let fit = |f: ImageFit, s: Size| f.fitted_rect(s, bounds, Alignment::TOP_LEADING, false);
    assert_eq!(fit(ImageFit::Fill, small), bounds);
    assert_eq!(
        fit(ImageFit::Contain, small),
        Rect::new(10.0, 20.0, 200.0, 50.0)
    );
    assert_eq!(
        fit(ImageFit::Cover, small),
        Rect::new(10.0, 20.0, 400.0, 100.0)
    );
    assert_eq!(
        fit(ImageFit::ScaleDown, small),
        Rect::new(10.0, 20.0, 40.0, 10.0),
        "never upscales"
    );
    assert_eq!(
        fit(ImageFit::ScaleDown, Size::new(800.0, 200.0)),
        Rect::new(10.0, 20.0, 200.0, 50.0),
        "but downscales like Contain"
    );
    assert_eq!(
        fit(ImageFit::None, Size::new(300.0, 30.0)),
        Rect::new(10.0, 20.0, 300.0, 30.0)
    );
    assert_eq!(
        fit(ImageFit::Contain, Size::new(0.0, 5.0)),
        bounds,
        "an empty picture fills the box"
    );
}

#[test]
fn alignment_places_the_slack_and_rtl_flips_leading() {
    let bounds = Rect::new(0.0, 0.0, 200.0, 100.0);
    let a = Alignment {
        horizontal: HAlignment::Trailing,
        vertical: VAlignment::Bottom,
    };
    let ltr = ImageFit::Contain.fitted_rect(Size::new(10.0, 10.0), bounds, a, false);
    assert_eq!((ltr.x, ltr.y), (100.0, 0.0));
    let rtl = ImageFit::Contain.fitted_rect(Size::new(10.0, 10.0), bounds, a, true);
    assert_eq!(rtl.x, 0.0);
}

#[test]
fn pixel_rect_arithmetic_never_overflows() {
    let edge = PixelRect::new(u32::MAX - 1, u32::MAX - 1, 10, 10);
    assert!(!edge.fits_in(u32::MAX, u32::MAX));
    assert!(!PixelRect::new(u32::MAX - 1, 0, 10, 1).fits_in(u32::MAX, 1));
    assert!(!PixelRect::new(0, u32::MAX - 1, 1, 10).fits_in(1, u32::MAX));
    assert!(PixelRect::new(u32::MAX, 0, 0, 1).fits_in(u32::MAX, 1));
    assert_eq!(edge.area(), 100);
    let u = edge.union(&PixelRect::new(0, 0, 1, 1));
    assert_eq!((u.x, u.y), (0, 0));
    assert_eq!((u.width, u.height), (u32::MAX, u32::MAX), "saturates");
    assert!(edge.intersect(&PixelRect::new(0, 0, 5, 5)).is_none());
}

#[test]
fn pixel_rect_union_and_intersection() {
    let a = PixelRect::new(2, 3, 4, 5);
    let b = PixelRect::new(5, 1, 4, 3);
    assert_eq!(a.union(&b), PixelRect::new(2, 1, 7, 7));
    assert_eq!(a.intersect(&b), Some(PixelRect::new(5, 3, 1, 1)));
    assert_eq!(a.union(&PixelRect::default()), a, "empty adds nothing");
    assert_eq!(PixelRect::default().union(&a), a);
    assert!(
        a.intersect(&PixelRect::new(6, 0, 3, 3)).is_none(),
        "edges touch"
    );
    assert!(PixelRect::full(3, 0).is_empty());
    assert_eq!(PixelRect::full(3, 4).area(), 12);
}

/// The spike's portrait case (spec §8.4): a 720 × 1280 frame in a 424 × 754
/// box under `Contain`, snapped at scale 1.
fn portrait() -> ImageGeometry {
    ImageGeometry::compute(
        Size::new(424.0, 754.0),
        (720, 1280),
        ImageFit::Contain,
        Alignment::CENTER,
        false,
        ImageOrientation::Normal,
    )
    .snapped(Transform2D::IDENTITY)
}

#[test]
fn the_portrait_frame_snaps_to_its_box() {
    let unsnapped = ImageGeometry::compute(
        Size::new(424.0, 754.0),
        (720, 1280),
        ImageFit::Contain,
        Alignment::CENTER,
        false,
        ImageOrientation::Normal,
    );
    assert!(approx(unsnapped.content.height, 753.778), "{unsnapped:?}");
    assert!(approx(unsnapped.content.y, 0.111), "{unsnapped:?}");
    assert_eq!(portrait().content, Rect::new(0.0, 0.0, 424.0, 754.0));
}

#[test]
fn the_portrait_frame_maps_as_section_8_4_says() {
    let g = portrait();
    // (point, map_to_source, map_to_source_clamped)
    type Row = ((f32, f32), Option<(u32, u32)>, (u32, u32));
    let rows: [Row; 5] = [
        ((0.0, 0.0), Some((0, 0)), (0, 0)),
        ((211.99, 376.99), Some((359, 639)), (359, 639)),
        ((423.99, 753.99), Some((719, 1279)), (719, 1279)),
        ((424.0, 377.0), None, (719, 640)),
        ((-5.0, -5.0), None, (0, 0)),
    ];
    for ((x, y), exact, clamped) in rows {
        let p = Point::new(x, y);
        assert_eq!(g.map_to_source(p), exact, "({x}, {y})");
        assert_eq!(
            g.map_to_source_clamped(p),
            Some(clamped),
            "({x}, {y}) clamped"
        );
    }
}

#[test]
fn a_rotated_camera_maps_as_section_8_4_says() {
    let g = ImageGeometry::compute(
        Size::new(424.0, 754.0),
        (1280, 720),
        ImageFit::Contain,
        Alignment::CENTER,
        false,
        ImageOrientation::Rotate90,
    )
    .snapped(Transform2D::IDENTITY);
    assert_eq!(g.displayed(), (720, 1280));
    assert_eq!(g.content, Rect::new(0.0, 0.0, 424.0, 754.0));
    for ((x, y), expected) in [
        ((0.0, 0.0), (0, 719)),
        ((211.99, 376.99), (639, 360)),
        ((423.99, 753.99), (1279, 0)),
    ] {
        assert_eq!(
            g.map_to_source(Point::new(x, y)),
            Some(expected),
            "({x}, {y})"
        );
    }
}

#[test]
fn the_far_edges_belong_to_no_pixel() {
    let g = portrait();
    assert_eq!(g.map_to_source(Point::new(424.0, 10.0)), None);
    assert_eq!(g.map_to_source(Point::new(10.0, 754.0)), None);
    assert_eq!(
        g.map_to_source_f32(Point::new(424.0, 754.0)),
        Some((720.0, 1280.0))
    );
}

#[test]
fn the_letterbox_maps_to_nothing_and_clamps_to_the_edge() {
    // A square picture in a wide box: bars on both sides.
    let g = ImageGeometry::compute(
        Size::new(300.0, 100.0),
        (10, 10),
        ImageFit::Contain,
        Alignment::CENTER,
        false,
        ImageOrientation::Normal,
    );
    assert_eq!(g.content, Rect::new(100.0, 0.0, 100.0, 100.0));
    assert_eq!(g.map_to_source(Point::new(50.0, 50.0)), None, "left bar");
    assert_eq!(g.map_to_source(Point::new(250.0, 50.0)), None, "right bar");
    // The picture's far edge lies inside the box here: it belongs to the
    // bar, not to the last column.
    assert_eq!(g.map_to_source(Point::new(200.0, 50.0)), None, "far edge");
    assert_eq!(g.map_to_source(Point::new(199.9, 50.0)), Some((9, 5)));
    assert_eq!(
        g.map_to_source_clamped(Point::new(50.0, 50.0)),
        Some((0, 5))
    );
    assert_eq!(
        g.map_to_source_clamped(Point::new(250.0, 99.0)),
        Some((9, 9))
    );
}

#[test]
fn a_cropped_picture_outside_the_box_is_not_hit() {
    // `Cover` crops a 2:1 picture in a square box: the cropped sides are
    // inside the content rect but outside the box.
    let g = ImageGeometry::compute(
        Size::new(100.0, 100.0),
        (20, 10),
        ImageFit::Cover,
        Alignment::CENTER,
        false,
        ImageOrientation::Normal,
    );
    assert_eq!(g.content, Rect::new(-50.0, 0.0, 200.0, 100.0));
    assert_eq!(g.map_to_source(Point::new(-10.0, 50.0)), None);
    assert_eq!(g.map_to_source(Point::new(0.0, 50.0)), Some((5, 5)));
    assert_eq!(g.visible(), Some(Rect::new(0.0, 0.0, 100.0, 100.0)));
}

/// C.9: every fit, both directions, every orientation and four device
/// scales. Each displayed pixel whose centre is in the box maps back to the
/// source pixel `apply_orientation` puts there, and `map_from_source` of
/// that pixel holds the centre.
#[test]
fn every_fit_orientation_direction_and_scale_maps_through_the_drawn_pixels() {
    let (w, h) = (6u32, 4u32);
    let boxes = [
        Size::new(18.0, 30.0),
        Size::new(40.0, 9.0),
        Size::new(5.0, 5.0),
    ];
    let alignments = [
        Alignment::CENTER,
        Alignment::TOP_LEADING,
        Alignment::BOTTOM_TRAILING,
    ];
    let mut checked = 0;
    for o in ALL {
        let (shown, dw, dh) = apply_orientation(coordinate_image(w, h), w, h, o);
        for fit in FITS {
            for rtl in [false, true] {
                for sf in [1.0f32, 1.25, 1.5, 2.0] {
                    for bounds in boxes {
                        for alignment in alignments {
                            let origin = Point::new(3.3, 7.9);
                            let to_device = Transform2D::translate(origin.x, origin.y)
                                .then(&Transform2D::scale(sf, sf));
                            let g = ImageGeometry::compute(bounds, (w, h), fit, alignment, rtl, o)
                                .snapped(to_device);
                            let c = g.content;
                            for yd in 0..dh {
                                for xd in 0..dw {
                                    let centre = Point::new(
                                        c.x + (xd as f32 + 0.5) * c.width / dw as f32,
                                        c.y + (yd as f32 + 0.5) * c.height / dh as f32,
                                    );
                                    let at = ((yd * dw + xd) * 4) as usize;
                                    let from = (u32::from(shown[at]), u32::from(shown[at + 1]));
                                    let inside = centre.x >= 0.0
                                        && centre.x < bounds.width
                                        && centre.y >= 0.0
                                        && centre.y < bounds.height;
                                    let ctx = format!(
                                        "{o:?} {fit:?} rtl={rtl} sf={sf} {bounds:?} {alignment:?} \
                                         displayed ({xd}, {yd})"
                                    );
                                    assert_eq!(
                                        g.map_to_source(centre),
                                        inside.then_some(from),
                                        "{ctx}"
                                    );
                                    let r = g
                                        .map_from_source(PixelRect::new(from.0, from.1, 1, 1))
                                        .unwrap();
                                    assert!(
                                        approx(r.x + r.width / 2.0, centre.x)
                                            && approx(r.y + r.height / 2.0, centre.y),
                                        "{ctx}: {r:?} against {centre:?}"
                                    );
                                    checked += usize::from(inside);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(
        checked > 10_000,
        "the sweep checked {checked} inside points"
    );
}

#[test]
fn map_from_source_bounds_a_turned_rect() {
    // A 4 × 2 source turned a quarter displays as 2 × 4; in a 20 × 40 box
    // each displayed pixel is 10 × 10.
    let g = ImageGeometry::compute(
        Size::new(20.0, 40.0),
        (4, 2),
        ImageFit::Fill,
        Alignment::CENTER,
        false,
        ImageOrientation::Rotate90,
    );
    // Source row 0, columns 1..=2, shows down the right-hand column.
    assert_eq!(
        g.map_from_source(PixelRect::new(1, 0, 2, 1)),
        Some(Rect::new(10.0, 10.0, 10.0, 20.0))
    );
    assert_eq!(
        g.map_from_source(PixelRect::new(3, 0, 2, 1)),
        None,
        "outside"
    );
    assert_eq!(g.map_from_source(PixelRect::new(0, 0, 0, 1)), None, "empty");
}

#[test]
fn nothing_to_show_maps_to_nothing() {
    let empty = ImageGeometry::compute(
        Size::new(10.0, 10.0),
        (0, 0),
        ImageFit::Contain,
        Alignment::CENTER,
        false,
        ImageOrientation::Normal,
    );
    let p = Point::new(5.0, 5.0);
    assert_eq!(empty.map_to_source(p), None);
    assert_eq!(empty.map_to_source_clamped(p), None);
    assert_eq!(empty.map_to_source_f32(p), None);
    let zero_box = ImageGeometry::compute(
        Size::new(0.0, 10.0),
        (4, 4),
        ImageFit::Contain,
        Alignment::CENTER,
        false,
        ImageOrientation::Normal,
    );
    assert_eq!(zero_box.map_to_source_clamped(p), None);
    assert_eq!(zero_box.map_from_source(PixelRect::full(4, 4)), None);
    assert_eq!(zero_box.visible(), None);
}

#[test]
fn snapping_puts_both_edges_on_the_device_grid() {
    let g = ImageGeometry::new(
        (10, 10),
        ImageOrientation::Normal,
        Rect::new(0.3, 0.6, 10.2, 9.7),
        Rect::new(0.0, 0.0, 20.0, 20.0),
    );
    // A widget at window (2.1, 0) on a 1.5× display.
    let to_device = Transform2D::translate(2.1, 0.0).then(&Transform2D::scale(1.5, 1.5));
    let s = g.snapped(to_device);
    for edge in [s.content.x, s.content.x + s.content.width] {
        let device = (edge + 2.1) * 1.5;
        assert!(approx(device, device.round()), "x edge {edge} → {device}");
    }
    for edge in [s.content.y, s.content.y + s.content.height] {
        let device = edge * 1.5;
        assert!(approx(device, device.round()), "y edge {edge} → {device}");
    }
    assert_eq!(
        s.snapped(to_device),
        s,
        "snapping twice changes nothing more"
    );
}

#[test]
fn snapping_follows_an_axis_aligned_ancestor_scale_and_skips_a_rotation() {
    let g = ImageGeometry::new(
        (10, 10),
        ImageOrientation::Normal,
        Rect::new(0.3, 0.3, 9.4, 9.4),
        Rect::new(0.0, 0.0, 10.0, 10.0),
    );
    // A `Scale(2.0)` ancestor: one local unit is two device pixels.
    let scaled = g.snapped(Transform2D::scale(2.0, 2.0));
    assert_eq!(scaled.content, Rect::new(0.5, 0.5, 9.0, 9.0));
    // Mirrored is still axis-aligned: edges stay edges.
    let mirrored = g.snapped(Transform2D::scale(-1.0, 1.0));
    assert_eq!(mirrored.content, Rect::new(0.0, 0.0, 10.0, 10.0));
    // A rotation takes edges off the grid's axes: no snapping.
    let rotated = g.snapped(Transform2D::rotate(0.3));
    assert_eq!(rotated, g);
    let singular = g.snapped(Transform2D::scale(0.0, 1.0));
    assert_eq!(singular, g);
}

#[test]
fn snapping_never_collapses_an_axis() {
    let g = ImageGeometry::new(
        (1, 1),
        ImageOrientation::Normal,
        Rect::new(0.1, 0.1, 0.2, 5.0),
        Rect::new(0.0, 0.0, 10.0, 10.0),
    );
    let s = g.snapped(Transform2D::IDENTITY);
    assert_eq!((s.content.x, s.content.width), (0.1, 0.2), "kept unsnapped");
    assert_eq!((s.content.y, s.content.height), (0.0, 5.0));
}

#[test]
fn an_uncropped_quad_samples_the_whole_texture_in_orientation_order() {
    let content = Rect::new(0.0, 0.0, 10.0, 20.0);
    let (rect, uv) = oriented_crop(content, content, ImageOrientation::Normal).unwrap();
    assert_eq!(rect, content);
    assert_eq!(uv, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    // Turned a quarter clockwise, the displayed top-leading corner shows the
    // source's bottom-left one.
    let (_, uv) = oriented_crop(content, content, ImageOrientation::Rotate90).unwrap();
    assert_eq!(uv, [[0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]);
}

#[test]
fn a_crop_samples_only_what_shows() {
    // Cover: the left and right quarters of a 2:1 picture are cropped.
    let content = Rect::new(-50.0, 0.0, 200.0, 100.0);
    let clip = Rect::new(0.0, 0.0, 100.0, 100.0);
    let (rect, uv) = oriented_crop(content, clip, ImageOrientation::Normal).unwrap();
    assert_eq!(rect, clip);
    assert_eq!(uv, [[0.25, 0.0], [0.75, 0.0], [0.75, 1.0], [0.25, 1.0]]);
    assert!(
        oriented_crop(
            content,
            Rect::new(500.0, 0.0, 10.0, 10.0),
            ImageOrientation::Normal
        )
        .is_none()
    );
}

/// Drawing and mapping agree: at each corner of every crop, the texture
/// coordinate is the mapped source point over the source size.
#[test]
fn each_crop_corner_samples_the_point_mapping_finds_there() {
    let (w, h) = (7u32, 3u32);
    for o in ALL {
        for fit in FITS {
            let g = ImageGeometry::compute(
                Size::new(30.0, 17.0),
                (w, h),
                fit,
                Alignment::BOTTOM_LEADING,
                false,
                o,
            );
            let Some((rect, uv)) = oriented_crop(g.content, g.bounds, o) else {
                continue;
            };
            let corners = [
                Point::new(rect.x, rect.y),
                Point::new(rect.x + rect.width, rect.y),
                Point::new(rect.x + rect.width, rect.y + rect.height),
                Point::new(rect.x, rect.y + rect.height),
            ];
            for (corner, [u, v]) in corners.into_iter().zip(uv) {
                let (sx, sy) = g.map_to_source_f32(corner).unwrap();
                assert!(
                    approx(u, sx / w as f32) && approx(v, sy / h as f32),
                    "{o:?} {fit:?} corner {corner:?}: uv ({u}, {v}) against ({sx}, {sy})"
                );
            }
        }
    }
}
