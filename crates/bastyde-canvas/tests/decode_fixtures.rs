// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Decoder tests against real encoder output.
//!
//! The unit tests in `raster.rs` build their inputs with the same crate that
//! reads them back, which can only ever prove the decoder is self-consistent.
//! Everything in this file was produced by an independent encoder, so it covers
//! the cases that actually break in the field: palette and 16-bit PNGs,
//! progressive and EXIF-rotated JPEGs, and files whose extension lies.

use bastyde_canvas::{ImageDecodeError, ImageFormat, RasterIcon};

fn fixture(name: &str) -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
    std::fs::read(format!("{path}{name}")).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

/// Every decode must yield exactly `width * height * 4` bytes — the contract
/// the atlas, the mip builder and the alpha-mask path all rely on.
fn assert_rgba_contract(icon: &RasterIcon, what: &str) {
    assert!(icon.width() > 0 && icon.height() > 0, "{what}: empty");
    assert_eq!(
        icon.pixels().len(),
        (icon.width() as usize) * (icon.height() as usize) * 4,
        "{what}: buffer is not tightly packed RGBA8"
    );
}

#[test]
fn sniffing_identifies_every_supported_format() {
    assert_eq!(
        ImageFormat::sniff(&fixture("rgba8.png")),
        Some(ImageFormat::Png)
    );
    assert_eq!(
        ImageFormat::sniff(&fixture("baseline.jpg")),
        Some(ImageFormat::Jpeg)
    );
    assert_eq!(
        ImageFormat::sniff(&fixture("progressive.jpg")),
        Some(ImageFormat::Jpeg)
    );
    assert_eq!(
        ImageFormat::sniff(&fixture("static.webp")),
        Some(ImageFormat::Webp)
    );
    assert_eq!(ImageFormat::sniff(b"nothing at all"), None);
    assert_eq!(ImageFormat::sniff(&[]), None);
    // A truncated RIFF header must not be read past.
    assert_eq!(ImageFormat::sniff(b"RIFF"), None);
}

#[test]
fn decode_dispatches_on_content_not_on_a_filename() {
    // The whole reason `decode` sniffs: a JPEG saved as ".png" is common, and
    // must open rather than fail.
    let jpeg_bytes = fixture("baseline.jpg");
    let icon = RasterIcon::decode(&jpeg_bytes).expect("sniffed as JPEG");
    assert_rgba_contract(&icon, "mislabelled jpeg");
    assert_eq!((icon.width(), icon.height()), (64, 48));
}

#[test]
fn unsupported_format_names_what_is_supported() {
    let err = RasterIcon::decode(b"GIF89a and then some").unwrap_err();
    assert_eq!(err, ImageDecodeError::UnsupportedFormat);
    // The message is shown to users unchanged, so it must list the formats.
    let msg = err.to_string();
    for f in ["PNG", "JPEG", "WebP"] {
        assert!(msg.contains(f), "{msg:?} should mention {f}");
    }
}

#[test]
fn palette_png_decodes_instead_of_being_rejected() {
    // This used to be a hard error ("re-export as RGBA"). Every screenshot
    // tool and every "save for web" path emits palette PNGs.
    let icon = RasterIcon::decode(&fixture("indexed.png")).expect("indexed PNG");
    assert_rgba_contract(&icon, "indexed png");
    assert_eq!((icon.width(), icon.height()), (32, 32));
    assert!(
        icon.pixels().chunks(4).all(|p| p[3] == 255),
        "an opaque palette PNG must decode fully opaque"
    );
}

#[test]
fn palette_png_with_trns_gets_a_real_alpha_channel() {
    let icon = RasterIcon::decode(&fixture("indexed_trns.png")).expect("tRNS PNG");
    assert_rgba_contract(&icon, "indexed+tRNS png");
    assert!(
        icon.pixels().chunks(4).any(|p| p[3] == 0),
        "the tRNS entry must expand into a transparent pixel"
    );
}

#[test]
fn sixteen_bit_png_is_stripped_to_eight_rather_than_misread() {
    // Without STRIP_16 the buffer is twice the expected length and the image
    // decodes as garbage at half width.
    let icon = RasterIcon::decode(&fixture("depth16.png")).expect("16-bit PNG");
    assert_rgba_contract(&icon, "16-bit png");
    assert_eq!((icon.width(), icon.height()), (16, 16));
}

#[test]
fn grayscale_and_interlaced_png_decode() {
    for name in ["grayscale.png", "interlaced.png", "rgba8.png"] {
        let icon = RasterIcon::decode(&fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_rgba_contract(&icon, name);
        assert_eq!((icon.width(), icon.height()), (32, 32), "{name}");
    }
}

#[test]
fn baseline_and_progressive_jpeg_decode_identically_sized() {
    let base = RasterIcon::decode(&fixture("baseline.jpg")).expect("baseline");
    let prog = RasterIcon::decode(&fixture("progressive.jpg")).expect("progressive");
    assert_rgba_contract(&base, "baseline jpeg");
    assert_rgba_contract(&prog, "progressive jpeg");
    assert_eq!((base.width(), base.height()), (64, 48));
    assert_eq!((prog.width(), prog.height()), (64, 48));
    // JPEG has no alpha: every pixel must come back fully opaque, or
    // compositing an inserted photo would blend the page through it.
    assert!(base.pixels().chunks(4).all(|p| p[3] == 255));
    assert!(prog.pixels().chunks(4).all(|p| p[3] == 255));
}

#[test]
fn grayscale_jpeg_decodes_to_rgba() {
    let icon = RasterIcon::decode(&fixture("grayscale.jpg")).expect("grayscale jpeg");
    assert_rgba_contract(&icon, "grayscale jpeg");
    // Grayscale means R == G == B for every pixel.
    for p in icon.pixels().chunks(4) {
        assert_eq!((p[0], p[1]), (p[1], p[2]), "not neutral: {p:?}");
    }
}

#[test]
fn exif_rotation_is_applied_at_decode_time() {
    // The source is 40×20 (landscape) with orientation 6 = rotate 90° CW, so a
    // correct decode reports 20×40 (portrait). A decoder that ignores EXIF
    // returns 40×20 and every phone photo shows up on its side.
    let icon = RasterIcon::decode(&fixture("exif_rotate90.jpg")).expect("exif jpeg");
    assert_rgba_contract(&icon, "exif jpeg");
    assert_eq!(
        (icon.width(), icon.height()),
        (20, 40),
        "EXIF orientation 6 must swap the axes"
    );

    // The left half of the original was red and the right half green; rotated
    // clockwise that becomes top red, bottom green.
    let px = icon.pixels();
    let at = |x: usize, y: usize| -> (u8, u8, u8) {
        let i = (y * 20 + x) * 4;
        (px[i], px[i + 1], px[i + 2])
    };
    let (tr, tg, _) = at(10, 5);
    let (br, bg, _) = at(10, 34);
    assert!(
        tr > 150 && tg < 110,
        "top should be red, got {:?}",
        at(10, 5)
    );
    assert!(
        bg > 150 && br < 110,
        "bottom should be green, got {:?}",
        at(10, 34)
    );
}

#[test]
fn cmyk_jpeg_decodes_fully_opaque() {
    // A 4-component (CMYK/YCCK) JPEG is what Adobe and print workflows emit.
    // The decoder's RGBA path leaves a colour channel in the alpha slot, so
    // without an explicit opaque pass the photo composites semi-transparently.
    let icon = RasterIcon::decode(&fixture("cmyk.jpg")).expect("cmyk jpeg");
    assert_rgba_contract(&icon, "cmyk jpeg");
    assert!(
        icon.pixels().chunks(4).all(|p| p[3] == 255),
        "CMYK JPEG must be fully opaque; found alpha {:?}",
        icon.pixels().chunks(4).map(|p| p[3]).find(|&a| a != 255)
    );
}

#[test]
fn static_webp_still_decodes() {
    let icon = RasterIcon::decode(&fixture("static.webp")).expect("webp");
    assert_rgba_contract(&icon, "webp");
    assert_eq!((icon.width(), icon.height()), (48, 32));
}

#[test]
fn truncating_any_fixture_never_panics() {
    // User-supplied files are routinely truncated by interrupted copies and
    // half-finished downloads. Every one of these must be an Err, not a crash.
    for name in [
        "baseline.jpg",
        "progressive.jpg",
        "exif_rotate90.jpg",
        "rgba8.png",
        "indexed.png",
        "depth16.png",
        "static.webp",
    ] {
        let full = fixture(name);
        for cut in [1, full.len() / 4, full.len() / 2, full.len() - 1] {
            let _ = RasterIcon::decode(&full[..cut]);
        }
    }
}

#[test]
fn downsample_bounds_the_long_edge_and_keeps_aspect() {
    let icon = RasterIcon::decode(&fixture("baseline.jpg")).expect("baseline");
    assert_eq!((icon.width(), icon.height()), (64, 48));

    let small = icon.downsample_to_max(32).expect("64 > 32, so it scales");
    assert_eq!((small.width(), small.height()), (32, 24));
    assert_rgba_contract(&small, "downsampled");

    // Already within bounds: no copy, no change.
    assert!(icon.downsample_to_max(64).is_none());
    assert!(icon.downsample_to_max(4096).is_none());
}

#[test]
fn downsampling_a_flat_image_does_not_shift_its_colour() {
    let icon = RasterIcon::from_raw([70u8, 130, 180, 255].repeat(100 * 100), 100, 100);
    let small = icon.downsample_to_max(17).expect("scales");
    assert_eq!((small.width(), small.height()), (17, 17));
    for p in small.pixels().chunks(4) {
        assert_eq!(p, &[70, 130, 180, 255]);
    }
}
