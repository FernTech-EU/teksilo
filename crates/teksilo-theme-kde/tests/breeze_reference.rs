// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Breeze controls rendered through the production wgpu renderer,
//! compared pixel by pixel with Plasma 6.8 painting the same controls
//! (`tools/kde_golden/vectors/pixels.json`, described in
//! `tools/kde_golden/README.md`).
//!
//! The unit tests beside each style check the colours and the rectangles it
//! asks for, and `tests/mount.rs` checks them in the render frame. Neither
//! sees where a 1 px outline lands on the pixel grid, how much of a corner
//! pixel a radius covers, how a 2 px check mark with mitred joins
//! rasterises, or which pixels a scroll-bar handle covers: those are decided
//! by the renderer from what the style asked for, and they are what this
//! file compares.
//!
//! ## How a control is rendered
//!
//! Each control is the root of a tree laid out at the size Plasma painted
//! it at (or, for the check box and the radio button, in the 24 px row the
//! widgets lay their 20 px indicator in, D8), with the text engine and the
//! glyph atlas the previewer's snapshots use (`teksilo-preview-ui`'s
//! `shot.rs`). It is rendered by `Renderer::render_capture` into an sRGB
//! texture cleared to the colour Plasma filled its image with, and read
//! back. Labels are empty: the vectors paint no text.
//!
//! ## How a pixel is compared
//!
//! Each sample names what it shows and is compared one of three ways:
//!
//! - **Solid**: a pixel wholly inside one opaque paint (a face, an outline
//!   that covers its pixel, the background). The colours must agree within
//!   [`SOLID_TOLERANCE`] on every channel.
//! - **Share**: an antialiased pixel, or a translucent paint. Each image's
//!   pixel is reduced to its share of the way from an `under` colour to an
//!   `over` colour (the projection onto the line between them), and the two
//!   shares must agree within [`SHARE_TOLERANCE`]. The share is measured in
//!   each image's own compositing space: Qt's raster painter blends in sRGB,
//!   Teksilo's renderer in linear light (it draws into an `Rgba8UnormSrgb`
//!   target). An edge covering 40 % of a pixel is 40 % of the way from one
//!   colour to the other in both, and a shadow at 12.5 % is 12.5 % of the way
//!   to black in both, although the 8-bit colours differ: compared as
//!   colours, every antialiased edge and every translucent paint over a
//!   backdrop the style does not paint would differ by the gap between the
//!   two spaces, which is up to 70 levels for a dark mark on a light face
//!   and says nothing about the style. Where the two colours are too close
//!   for one level to leave the share meaningful ([`MAX_SHARE_STEP`]), the
//!   pixel is compared as a solid one.
//! - **Bare** and **Ink**: the pixel is the background in both images, or
//!   covered in both. Only for the scroll-bar handle and the unmarked check
//!   box's outline, whose colours the preset lifts (D9, D6).
//!
//! The `under` and `over` colours are either a pixel of the same image (so a
//! colour the preset deliberately lifts, D6 or D9, is measured against its
//! own lifted value) or a palette colour, the same in both.
//!
//! ## The tolerances
//!
//! - [`SOLID_TOLERANCE`], 2 levels: the colours are KDE's 16-bit
//!   arithmetic rounded to 8 bits, then linearised and re-encoded by the
//!   texture, each step within one level.
//! - [`SHARE_TOLERANCE`], the sum of two measured gaps, each pinned by a
//!   test below. [`RENDERER_COVERAGE_GAP`]: the renderer's shape pipeline
//!   and its path atlas ramp a pixel's coverage with a smoothstep over the
//!   signed distance at its centre, where the area of the shape inside the
//!   pixel is the true coverage; on a straight edge the two differ by up to
//!   0.096, and on the arcs sampled here by up to 0.093. [`QT_CORNER_GAP`]:
//!   Qt's own corners are not the area of the specification's shape either,
//!   by up to 0.12 at the push button's (its arcs lie a little outside the
//!   radius). The two can add. Half a pixel of radius still moves the share
//!   of the corner pixel on the diagonal by 0.3 or more, a missing square
//!   cap the pixel before the check mark by 0.33, a misplaced outline by 1.
//!
//! One pixel is left out for that reason: the inner edge of the checked
//! box's corner, (5, 3), where the outline is laid twice and the renderer's
//! gap doubles with it. The doubled outline also saturates the box's corner
//! pixels, so the box's radius is held from below only: at 3.5 its corners
//! disagree, at 4.5 they do not.
//!
//! ## Where the two bars differ
//!
//! The scroll bar's handle is filled in the lifted thumb colour (D9), so it
//! is compared by which pixels it covers, not by its colour; and Breeze
//! starts it 3 px down the bar, the margin it keeps at each end of a bar
//! without buttons, which the widget leaves its style no say over (the
//! crate's known limitations): the handle samples are compared 3 px apart.
//!
//! A host with no GPU adapter skips every test here with a message (unless
//! `TEKSILO_TEST_REQUIRE_ADAPTER` asks for one). This file reads the vectors
//! from the repository, so it is left out of the published package (the
//! `exclude` list in `Cargo.toml`).

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::rc::Rc;

use serde_json::Value;
use teksilo_canvas::SizeProposal;
use teksilo_core::event::PointerButton;
use teksilo_core::event_source::TreeAppContext;
use teksilo_core::signal::Signal;
use teksilo_core::styles::Theme;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_kde_scheme::{ColorState, PaletteRole};
use teksilo_render::test_support;
use teksilo_text::SharedTypesetter;
use teksilo_theme_kde::{self as kde, KdePalette};
use teksilo_tokens::Color;
use teksilo_widgets::scroll_bar::{ScrollBar, ScrollBarOrientation, ScrollBarVariant};
use teksilo_widgets::{Button, ButtonVariant, CheckState, Checkbox, RadioButton, StandardListItem};

const PIXELS_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/kde_golden/vectors/pixels.json"
));

/// The largest per-channel difference between two solid pixels.
const SOLID_TOLERANCE: u8 = 2;

/// The most the renderer's coverage of a pixel departs from the area its
/// shape covers: `max |smoothstep(t) - t|` over a one-pixel ramp,
/// [`the_renderer_coverage_gap_is_the_smoothsteps`].
const RENDERER_COVERAGE_GAP: f32 = 0.097;

/// The most Qt's coverage of a push button's corner pixel departs from the
/// area of the specification's shape (a 1 px outline at radius 4.5),
/// [`qts_corners_depart_from_the_specified_shape_by_the_qt_gap`].
const QT_CORNER_GAP: f32 = 0.12;

/// The largest difference between two shares. See the module documentation.
const SHARE_TOLERANCE: f32 = RENDERER_COVERAGE_GAP + QT_CORNER_GAP;

/// The most one 8-bit level of a pixel may move its share for the share to
/// be measured. Where the `under` and `over` colours are closer than that
/// (a shadow at 12.5 % over Breeze Dark's window, which is 32 levels from
/// black in sRGB but 0.015 from it in linear light), the pixel is compared as
/// a colour instead: the two compositing spaces cannot then differ by more
/// than [`SOLID_TOLERANCE`].
const MAX_SHARE_STEP: f32 = SHARE_TOLERANCE / 4.0;

/// How far a pixel must be from the background, in levels on some channel,
/// to count as covered by the scroll-bar handle.
const INK_THRESHOLD: u8 = 8;

// ── The vectors ─────────────────────────────────────────────────────────

/// One of Plasma's painted controls: its image size, the colour the image
/// was filled with, and the sampled rows and columns.
struct Golden {
    size: (usize, usize),
    background: [u8; 4],
    rows: HashMap<usize, Vec<(usize, usize, [u8; 4])>>,
    cols: HashMap<usize, Vec<(usize, usize, [u8; 4])>>,
}

fn hex(s: &str) -> [u8; 4] {
    let h = s.trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).expect("a hex colour");
    [byte(0), byte(2), byte(4), byte(6)]
}

fn lines(value: &Value) -> HashMap<usize, Vec<(usize, usize, [u8; 4])>> {
    value
        .as_object()
        .expect("sampled lines")
        .iter()
        .map(|(at, runs)| {
            let runs = runs
                .as_array()
                .expect("a run-length line")
                .iter()
                .map(|run| {
                    (
                        run[0].as_u64().expect("a start") as usize,
                        run[1].as_u64().expect("a length") as usize,
                        hex(run[2].as_str().expect("a colour")),
                    )
                })
                .collect();
            (at.parse().expect("a line index"), runs)
        })
        .collect()
}

impl Golden {
    fn load(scheme: &str, id: &str) -> Golden {
        let all: Value = serde_json::from_str(PIXELS_JSON).expect("pixels.json parses");
        let record = all["schemes"][scheme]["records"]
            .as_array()
            .expect("the scheme's records")
            .iter()
            .find(|r| r["id"] == id)
            .unwrap_or_else(|| panic!("{scheme} has no {id}"));
        let size = record["size"].as_array().expect("a size");
        Golden {
            size: (
                size[0].as_u64().unwrap() as usize,
                size[1].as_u64().unwrap() as usize,
            ),
            background: hex(record["background"].as_str().expect("a background")),
            rows: lines(&record["rows"]),
            cols: lines(&record["cols"]),
        }
    }

    /// The colour Plasma painted at `(x, y)`, which must lie on a sampled
    /// row or column.
    fn at(&self, x: usize, y: usize) -> [u8; 4] {
        let (runs, along) = if let Some(row) = self.rows.get(&y) {
            (row, x)
        } else if let Some(col) = self.cols.get(&x) {
            (col, y)
        } else {
            panic!("neither row {y} nor column {x} is sampled");
        };
        runs.iter()
            .find(|(start, len, _)| (*start..start + len).contains(&along))
            .map(|(_, _, c)| *c)
            .expect("the line covers the pixel")
    }
}

// ── The renderer ────────────────────────────────────────────────────────

/// The production renderer on the shared offscreen device, with the text
/// engine and glyph atlas a snapshot needs.
struct Gpu {
    renderer: teksilo_render::Renderer,
    device: wgpu::Device,
    queue: wgpu::Queue,
    typesetter: SharedTypesetter,
    atlas_version: u64,
}

impl Gpu {
    /// The renderer, or `None` with a message when this host has no adapter.
    fn new(test: &str) -> Option<Gpu> {
        let Some((renderer, device, queue)) =
            pollster::block_on(test_support::require_test_renderer("breeze_reference"))
        else {
            println!("{test}: skipped, no GPU adapter on this host");
            return None;
        };
        Some(Gpu {
            renderer,
            device,
            queue,
            typesetter: SharedTypesetter::new_with_default_font(),
            atlas_version: 0,
        })
    }

    /// A tree under `theme` with the typesetter as its text backend and as
    /// app state, where the editable text widgets look for it.
    fn tree(&self, theme: Theme) -> WidgetTree {
        let mut tree = WidgetTree::new()
            .with_theme(theme)
            .with_text_backend(self.typesetter.as_text_backend());
        let mut app_state: HashMap<TypeId, Box<dyn Any>> = HashMap::new();
        app_state.insert(
            TypeId::of::<SharedTypesetter>(),
            Box::new(self.typesetter.clone()),
        );
        tree.set_app_context(Rc::new(TreeAppContext::empty().with_app_state(app_state)));
        tree
    }

    /// Render `tree`, laid out at `width` x `height`, on a target cleared to
    /// `background`: tightly packed RGBA.
    fn capture(
        &mut self,
        tree: &mut WidgetTree,
        width: u32,
        height: u32,
        background: [u8; 4],
    ) -> Capture {
        tree.layout(SizeProposal::exact(width as f32, height as f32));
        let frame = tree.render();
        // Glyphs rasterise during the render above; the renderer only knows
        // them once the atlas is uploaded.
        let atlas = self
            .typesetter
            .bridge()
            .borrow_mut()
            .atlas_info(self.atlas_version);
        if !atlas.pixels.is_empty() && atlas.width > 0 && atlas.height > 0 {
            self.renderer
                .upload_atlas(atlas.width, atlas.height, &atlas.pixels);
            self.atlas_version = atlas.version;
        }
        let clear =
            teksilo_render::vertex::srgb_to_linear_rgba(background.map(|v| f32::from(v) / 255.0));
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("breeze_reference"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.renderer
            .render_capture(&frame, &view, 1.0, width, height, clear);
        Capture {
            width: width as usize,
            rgba: test_support::read_texture_rgba(
                &self.device,
                &self.queue,
                &texture,
                width,
                height,
            ),
        }
    }
}

/// A rendered image.
struct Capture {
    width: usize,
    rgba: Vec<u8>,
}

impl Capture {
    fn at(&self, x: usize, y: usize) -> [u8; 4] {
        let i = (y * self.width + x) * 4;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }
}

// ── The checks ──────────────────────────────────────────────────────────

/// A colour a share is measured from or toward.
#[derive(Debug, Clone, Copy)]
enum Ref {
    /// The pixel at `(x, y)` of the golden image, and the matching pixel of
    /// the capture.
    At(usize, usize),
    /// A colour of the scheme's active palette, the same in both images.
    Role(PaletteRole),
    /// Black, the shadow colour.
    Black,
}

#[derive(Debug, Clone, Copy)]
enum Check {
    /// A pixel inside one opaque paint: the colours agree.
    Solid,
    /// An antialiased pixel or a translucent paint: the shares from `under`
    /// to `over` agree.
    Share { under: Ref, over: Ref },
    /// The pixel is the background in both images (scroll-bar handle).
    Bare,
    /// The pixel is covered, not the background, in both images.
    Ink,
}

/// One compared pixel: what it shows, where it is in the golden image, and
/// how it is compared.
#[derive(Debug, Clone, Copy)]
struct Sample {
    what: &'static str,
    at: (usize, usize),
    check: Check,
    /// Added to the capture's coordinates: where a control places a part
    /// elsewhere than Plasma does, by design (see the module documentation).
    shift: (isize, isize),
}

fn solid(what: &'static str, x: usize, y: usize) -> Sample {
    Sample {
        what,
        at: (x, y),
        check: Check::Solid,
        shift: (0, 0),
    }
}

fn share(what: &'static str, x: usize, y: usize, under: Ref, over: Ref) -> Sample {
    Sample {
        what,
        at: (x, y),
        check: Check::Share { under, over },
        shift: (0, 0),
    }
}

/// `pixels` as shares of the way from `under` to `over`.
fn shares(what: &'static str, pixels: &[(usize, usize)], under: Ref, over: Ref) -> Vec<Sample> {
    pixels
        .iter()
        .map(|&(x, y)| share(what, x, y, under, over))
        .collect()
}

/// `pixels` checked as covered or bare.
fn coverage(what: &'static str, pixels: &[(usize, usize)], check: Check) -> Vec<Sample> {
    pixels
        .iter()
        .map(|&(x, y)| Sample {
            what,
            at: (x, y),
            check,
            shift: (0, 0),
        })
        .collect()
}

/// `pixels` as solid pixels.
fn solids(what: &'static str, pixels: &[(usize, usize)]) -> Vec<Sample> {
    pixels.iter().map(|&(x, y)| solid(what, x, y)).collect()
}

fn unit(c: [u8; 4], linear: bool) -> [f32; 3] {
    let f = [c[0], c[1], c[2], 255].map(|v| f32::from(v) / 255.0);
    let f = if linear {
        teksilo_render::vertex::srgb_to_linear_rgba(f)
    } else {
        f
    };
    [f[0], f[1], f[2]]
}

/// `pixel`'s share of the way from `under` to `over`, in sRGB or in linear
/// light, and how much it moves when the pixel moves one 8-bit level on
/// every channel; `None` when the two colours are the same.
fn share_of(pixel: [u8; 4], under: [u8; 4], over: [u8; 4], linear: bool) -> Option<(f32, f32)> {
    let (p, u, o) = (unit(pixel, linear), unit(under, linear), unit(over, linear));
    let d = [o[0] - u[0], o[1] - u[1], o[2] - u[2]];
    let span = d.iter().map(|v| v * v).sum::<f32>();
    if span <= f32::EPSILON {
        return None;
    }
    let share = (0..3).map(|i| (p[i] - u[i]) * d[i]).sum::<f32>() / span;
    let up = pixel.map(|v| v.saturating_add(1));
    let q = unit(up, linear);
    let step = (0..3).map(|i| ((q[i] - p[i]) * d[i]).abs()).sum::<f32>() / span;
    Some((share, step))
}

fn rgb(c: [u8; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn differs(a: [u8; 4], b: [u8; 4], by: u8) -> bool {
    (0..3).any(|i| a[i].abs_diff(b[i]) > by)
}

// ── The scenarios ───────────────────────────────────────────────────────

/// One golden record and the control rendered to match it.
struct Scenario {
    /// The record's id in `pixels.json`.
    id: &'static str,
    /// The capture's size.
    canvas: (u32, u32),
    /// Where the golden image's origin lies in the capture.
    origin: (usize, usize),
    /// Mount the control as the tree's root.
    mount: fn(&mut WidgetTree) -> WidgetId,
    /// Put it in the record's state, after a first layout.
    prepare: fn(&mut WidgetTree, WidgetId),
    samples: Vec<Sample>,
}

fn at_rest(_: &mut WidgetTree, _: WidgetId) {}

fn hovered(tree: &mut WidgetTree, id: WidgetId) {
    let centre = tree.bounds(id).center();
    tree.pointer_move(centre);
}

fn pressed(tree: &mut WidgetTree, id: WidgetId) {
    let centre = tree.bounds(id).center();
    tree.pointer_move(centre);
    tree.pointer_down_button(centre, PointerButton::Primary);
}

fn palette_colour(theme: &Theme, role: PaletteRole) -> [u8; 4] {
    let c: Color = theme
        .extension::<KdePalette>()
        .expect("the preset's palette")
        .role(ColorState::Active, role);
    c.to_array()
        .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// Render every scenario in Breeze Light and Breeze Dark and compare its
/// samples: the report of every sample that disagrees, empty when all agree.
fn compare(gpu: &mut Gpu, scenarios: &[Scenario]) -> Vec<String> {
    let mut failures = Vec::new();
    for (scheme, theme) in [("BreezeLight", kde::light()), ("BreezeDark", kde::dark())] {
        for s in scenarios {
            let golden = Golden::load(scheme, s.id);
            let mut tree = gpu.tree(theme.clone());
            let id = (s.mount)(&mut tree);
            tree.layout(SizeProposal::exact(s.canvas.0 as f32, s.canvas.1 as f32));
            (s.prepare)(&mut tree, id);
            let shot = gpu.capture(&mut tree, s.canvas.0, s.canvas.1, golden.background);
            let ours = |(x, y): (usize, usize), shift: (isize, isize)| {
                let gx = (x + s.origin.0) as isize + shift.0;
                let gy = (y + s.origin.1) as isize + shift.1;
                assert!(
                    gx >= 0 && gy >= 0 && (gx as u32) < s.canvas.0 && (gy as u32) < s.canvas.1,
                    "{}: ({x}, {y}) lies outside the capture",
                    s.id
                );
                shot.at(gx as usize, gy as usize)
            };
            let colour = |r: Ref, shift: (isize, isize)| match r {
                Ref::At(x, y) => (golden.at(x, y), ours((x, y), shift)),
                Ref::Role(role) => {
                    let c = palette_colour(&theme, role);
                    (c, c)
                }
                Ref::Black => ([0, 0, 0, 255], [0, 0, 0, 255]),
            };
            assert!(
                golden.size.0 + s.origin.0 <= s.canvas.0 as usize
                    && golden.size.1 + s.origin.1 <= s.canvas.1 as usize,
                "{}: Plasma's {:?} image does not fit the capture",
                s.id,
                golden.size
            );
            for sample in &s.samples {
                let (x, y) = sample.at;
                let theirs = golden.at(x, y);
                let mine = ours((x, y), sample.shift);
                let fail = match sample.check {
                    Check::Solid => differs(mine, theirs, SOLID_TOLERANCE)
                        .then(|| format!("{} against Plasma's {}", rgb(mine), rgb(theirs))),
                    Check::Share { under, over } => {
                        let (u_theirs, u_mine) = colour(under, sample.shift);
                        let (o_theirs, o_mine) = colour(over, sample.shift);
                        let measured = |p, u, o, linear| {
                            share_of(p, u, o, linear).unwrap_or_else(|| {
                                panic!(
                                    "{} {}: {under:?} and {over:?} are one colour",
                                    s.id, sample.what
                                )
                            })
                        };
                        let (t, t_step) = measured(theirs, u_theirs, o_theirs, false);
                        let (m, m_step) = measured(mine, u_mine, o_mine, true);
                        if t_step.max(m_step) > MAX_SHARE_STEP {
                            differs(mine, theirs, SOLID_TOLERANCE).then(|| {
                                format!(
                                    "{} against Plasma's {} (too close to {under:?} for a share)",
                                    rgb(mine),
                                    rgb(theirs)
                                )
                            })
                        } else {
                            ((t - m).abs() > SHARE_TOLERANCE).then(|| {
                                format!(
                                    "share {m:.3} ({}) against Plasma's {t:.3} ({})",
                                    rgb(mine),
                                    rgb(theirs)
                                )
                            })
                        }
                    }
                    Check::Bare => (differs(theirs, golden.background, SOLID_TOLERANCE)
                        || differs(mine, golden.background, SOLID_TOLERANCE))
                    .then(|| {
                        format!(
                            "{} and Plasma's {} are not both the background",
                            rgb(mine),
                            rgb(theirs)
                        )
                    }),
                    Check::Ink => (!differs(theirs, golden.background, INK_THRESHOLD)
                        || !differs(mine, golden.background, INK_THRESHOLD))
                    .then(|| {
                        format!(
                            "{} and Plasma's {} are not both covered",
                            rgb(mine),
                            rgb(theirs)
                        )
                    }),
                };
                if let Some(why) = fail {
                    failures.push(format!(
                        "{scheme} {} {} at ({x}, {y}): {why}",
                        s.id, sample.what
                    ));
                }
            }
        }
    }
    failures
}

fn assert_agree(test: &str, scenarios: Vec<Scenario>) {
    let Some(mut gpu) = Gpu::new(test) else {
        return;
    };
    let failures = compare(&mut gpu, &scenarios);
    assert!(
        failures.is_empty(),
        "{} samples disagree with Plasma:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ── Push buttons ────────────────────────────────────────────────────────

/// The push button's frame, as a list of samples: the outline on all four
/// sides lands on the pixel 1 px inside the node (the frame inset by 1 for
/// the shadow, stroked on the rectangle inset by half the pen), the face
/// fills what it encloses, nothing is painted outside it, and the corners
/// round at 4.5 on the outline's centre line.
fn button_frame() -> Vec<Sample> {
    let face = Ref::At(42, 17);
    let top = Ref::At(42, 1);
    let leading = Ref::At(1, 17);
    let mut s = vec![
        solid("face", 42, 17),
        solid("leading outline", 1, 17),
        solid("trailing outline", 82, 17),
        solid("top outline", 42, 1),
        solid("bottom outline", 42, 32),
        solid("outside, leading", 0, 17),
        solid("outside, trailing", 83, 17),
        solid("outside, above", 42, 0),
    ];
    s.extend(shares(
        "top-leading arc, outer edge",
        &[(3, 1), (4, 1), (5, 1)],
        Ref::At(0, 1),
        top,
    ));
    s.extend(shares(
        "top-trailing arc, outer edge",
        &[(78, 1), (79, 1), (80, 1)],
        Ref::At(83, 1),
        top,
    ));
    s.extend(shares(
        "leading arc, outer edge",
        &[(1, 3), (1, 4), (1, 5)],
        Ref::At(1, 0),
        leading,
    ));
    s.extend(shares(
        "arc on the diagonal",
        &[(2, 2), (81, 2)],
        Ref::At(0, 2),
        top,
    ));
    s.extend(shares(
        "top-leading arc, inner edge",
        &[(4, 2), (5, 2)],
        face,
        top,
    ));
    s.extend(shares(
        "top-trailing arc, inner edge",
        &[(78, 2), (79, 2)],
        face,
        top,
    ));
    s.extend(shares(
        "leading arc, inner edge",
        &[(2, 4), (2, 5)],
        face,
        leading,
    ));
    s
}

/// The 1 px shadow below the frame: black at 12.5 % over the window.
fn button_shadow() -> Vec<Sample> {
    shares(
        "shadow line",
        &[(42, 33), (20, 33), (70, 33)],
        Ref::At(0, 33),
        Ref::Black,
    )
}

fn push_button(
    id: &'static str,
    mount: fn(&mut WidgetTree) -> WidgetId,
    prepare: fn(&mut WidgetTree, WidgetId),
    shadow: bool,
) -> Scenario {
    let mut samples = button_frame();
    if shadow {
        samples.extend(button_shadow());
    } else {
        samples.push(solid("no shadow", 42, 33));
    }
    Scenario {
        id,
        canvas: (84, 34),
        origin: (0, 0),
        mount,
        prepare,
        samples,
    }
}

fn plain_button(tree: &mut WidgetTree) -> WidgetId {
    tree.add(Button::new(lit!("")))
}

fn default_button(tree: &mut WidgetTree) -> WidgetId {
    tree.add(Button::new(lit!("")).variant(ButtonVariant::Filled))
}

/// The push button at rest, hovered, pressed and as the default button: the
/// frame's outline and radius in each colour, the shadow where Plasma casts
/// it and nothing below a pressed button.
#[test]
fn push_buttons_land_on_plasmas_pixels() {
    assert_agree(
        "push_buttons_land_on_plasmas_pixels",
        vec![
            push_button("push_button.normal", plain_button, at_rest, true),
            push_button("push_button.hover", plain_button, hovered, true),
            push_button("push_button.pressed", plain_button, pressed, false),
            push_button("push_button.default", default_button, at_rest, true),
        ],
    );
}

// ── Check boxes and radio buttons ───────────────────────────────────────

/// Where the 20 px indicator lies in the 24 px row the widgets lay it out in
/// (D8): 2 px down.
const INDICATOR_ORIGIN: (usize, usize) = (0, 2);
const INDICATOR_CANVAS: (u32, u32) = (20, 24);

/// The box: its outline on the pixel 2 px inside the indicator on every
/// side (the 16 px box stroked at 2.5), and nothing outside it. A marked
/// box's outline is Plasma's colour, so its corners are compared too; an
/// unmarked box's outline is lifted (D6), so only which pixels it covers is.
fn box_frame(marked: bool) -> Vec<Sample> {
    let mut s = solids("outside the box", &[(0, 10), (19, 10), (8, 0), (10, 19)]);
    let edge = [(2, 8), (17, 8), (8, 2), (8, 17)];
    if !marked {
        s.extend(coverage("box outline", &edge, Check::Ink));
        s.extend(coverage(
            "just outside the outline",
            &[(1, 8), (18, 8), (8, 1), (8, 18)],
            Check::Bare,
        ));
        return s;
    }
    s.extend(solids("box outline", &edge));
    s.extend(shares(
        "box corner, outer edge",
        &[(4, 2), (5, 2), (14, 2), (15, 2)],
        Ref::At(0, 2),
        Ref::At(8, 2),
    ));
    s.extend(shares(
        "box corner, outer edge",
        &[(2, 4), (2, 5), (2, 14), (2, 15)],
        Ref::At(2, 0),
        Ref::At(2, 8),
    ));
    s.extend(shares(
        "box corner on the diagonal",
        &[(3, 3), (16, 3)],
        Ref::At(0, 3),
        Ref::At(8, 2),
    ));
    s
}

fn checked_box(tree: &mut WidgetTree) -> WidgetId {
    tree.add(Checkbox::new(Signal::new(true)))
}

fn unchecked_box(tree: &mut WidgetTree) -> WidgetId {
    tree.add(Checkbox::new(Signal::new(false)))
}

fn partial_box(tree: &mut WidgetTree) -> WidgetId {
    tree.add(Checkbox::tristate(Signal::new(CheckState::Indeterminate)))
}

/// A checked box: the frame, the highlight wash over the face, and the
/// check mark, a 2 px polyline with mitred joins through (6, 10), (9, 13)
/// and (14.5, 7.5) of the indicator: the pixels it covers wholly, and its
/// antialiased edges, the square caps past both ends among them.
fn checked_samples() -> Vec<Sample> {
    let wash = Ref::At(8, 5);
    let mark = Ref::At(13, 8);
    let mut s = box_frame(true);
    s.extend(solids("wash", &[(8, 5), (3, 8), (16, 12), (14, 10)]));
    // Not (5, 3) and (14, 3), where the arc's inner edge crosses the pixel:
    // the outline is laid twice there, which doubles the gap between the two
    // coverage models with it (0.16 at that pixel, see the module
    // documentation), while the outer edge and the pixels on either side of
    // the inner one stay within the tolerance.
    s.extend(shares(
        "box corner, inner edge",
        &[(6, 3), (13, 3), (4, 3), (15, 3)],
        wash,
        Ref::At(8, 2),
    ));
    s.extend(solids(
        "check mark",
        &[(13, 8), (6, 10), (11, 10), (8, 12), (9, 12), (10, 11)],
    ));
    s.extend(shares(
        "check mark edge",
        &[
            (12, 8),
            (14, 8),
            (11, 8),
            (15, 8),
            (7, 10),
            (8, 10),
            (10, 10),
            (12, 10),
            (13, 10),
            (7, 12),
            (10, 12),
            (6, 12),
            (11, 12),
            (8, 11),
            (8, 13),
            (8, 14),
            (10, 9),
            (10, 12),
            (10, 13),
        ],
        wash,
        mark,
    ));
    s.extend(shares(
        "check mark, square cap at the start",
        &[(4, 10), (5, 10)],
        wash,
        mark,
    ));
    s
}

/// A partial box: three 2 x 2 squares of the view's text on the box's
/// centre line, 4 px apart.
fn partial_samples() -> Vec<Sample> {
    let mut s = box_frame(true);
    s.extend(solids(
        "partial square",
        &[
            (5, 10),
            (6, 10),
            (9, 10),
            (10, 10),
            (13, 10),
            (14, 10),
            (10, 9),
        ],
    ));
    s.extend(solids(
        "between the squares",
        &[
            (7, 10),
            (8, 10),
            (11, 10),
            (12, 10),
            (10, 8),
            (10, 11),
            (8, 5),
        ],
    ));
    s
}

/// An unchecked box: the face, and the frame's geometry.
fn unchecked_samples() -> Vec<Sample> {
    let mut s = box_frame(false);
    s.extend(solids("face", &[(8, 8), (10, 10), (3, 8), (16, 12)]));
    s
}

fn selected_radio(tree: &mut WidgetTree) -> WidgetId {
    tree.add(RadioButton::new(0, Signal::new(0)))
}

fn unselected_radio(tree: &mut WidgetTree) -> WidgetId {
    tree.add(RadioButton::new(1, Signal::new(0)))
}

/// The disc: the 15 px circle at 2.5 inside the indicator, outlined on the
/// pixel 2 px inside it on every side, and, selected, its arc. Unselected,
/// the outline is lifted (D6), as the box's is.
fn disc_frame(marked: bool) -> Vec<Sample> {
    let mut s = solids("outside the disc", &[(0, 10), (19, 10), (10, 0), (10, 19)]);
    let edge = [(2, 10), (17, 10), (10, 2), (10, 17)];
    if !marked {
        s.extend(coverage("disc outline", &edge, Check::Ink));
        s.extend(coverage(
            "just outside the outline",
            &[(1, 10), (18, 10), (10, 1), (10, 18)],
            Check::Bare,
        ));
        return s;
    }
    s.extend(solids("disc outline", &edge));
    s.extend(shares(
        "disc arc, outer edge",
        &[(4, 3), (5, 3), (14, 3), (15, 3)],
        Ref::At(0, 3),
        Ref::At(10, 2),
    ));
    s
}

/// A selected radio button: the disc, the wash and the 6 px dot.
fn selected_radio_samples() -> Vec<Sample> {
    let wash = Ref::At(5, 10);
    let dot = Ref::At(10, 10);
    let mut s = disc_frame(true);
    s.extend(solids("wash", &[(5, 10), (14, 10), (10, 5), (10, 14)]));
    s.extend(shares(
        "disc arc, inner edge",
        &[
            (3, 10),
            (16, 10),
            (10, 3),
            (10, 16),
            (7, 3),
            (8, 3),
            (12, 3),
        ],
        wash,
        Ref::At(10, 2),
    ));
    s.extend(solids("dot", &[(9, 10), (10, 10), (10, 8), (10, 11)]));
    s.extend(shares(
        "dot edge",
        &[(7, 10), (12, 10), (10, 7), (10, 12)],
        wash,
        dot,
    ));
    s
}

/// An unselected radio button: the face and the disc's geometry.
fn unselected_radio_samples() -> Vec<Sample> {
    let mut s = disc_frame(false);
    s.extend(solids("face", &[(10, 10), (5, 10), (10, 14)]));
    s
}

fn indicator(
    id: &'static str,
    mount: fn(&mut WidgetTree) -> WidgetId,
    samples: Vec<Sample>,
) -> Scenario {
    Scenario {
        id,
        canvas: INDICATOR_CANVAS,
        origin: INDICATOR_ORIGIN,
        mount,
        prepare: at_rest,
        samples,
    }
}

/// The check box checked, partial and unchecked, and the radio button
/// selected and not: the box and the disc where Plasma puts them, at its
/// radii, the wash, the check mark, the partial squares and the dot.
#[test]
fn check_boxes_and_radio_buttons_land_on_plasmas_pixels() {
    assert_agree(
        "check_boxes_and_radio_buttons_land_on_plasmas_pixels",
        vec![
            indicator("check_box.on", checked_box, checked_samples()),
            indicator("check_box.partial", partial_box, partial_samples()),
            indicator("check_box.off", unchecked_box, unchecked_samples()),
            indicator("radio_button.on", selected_radio, selected_radio_samples()),
            indicator(
                "radio_button.off",
                unselected_radio,
                unselected_radio_samples(),
            ),
        ],
    );
}

// ── List rows ───────────────────────────────────────────────────────────

fn plain_row(tree: &mut WidgetTree) -> WidgetId {
    tree.add(StandardListItem::new(lit!("")))
}

fn selected_row(tree: &mut WidgetTree) -> WidgetId {
    tree.add(StandardListItem::new(lit!("")).selected(Signal::new(true)))
}

/// The selection's rectangle: 2 px inside the row's leading and trailing
/// edges and 1 px inside its top and bottom, nothing outside it, and its
/// four rounded corners.
fn selection_frame() -> Vec<Sample> {
    let mut s = solids(
        "outside the selection",
        &[(1, 12), (198, 12), (100, 0), (100, 23), (0, 1), (199, 22)],
    );
    s.extend(shares(
        "top-leading corner",
        &[(4, 1), (5, 1), (6, 1), (3, 2), (4, 2)],
        Ref::At(0, 1),
        Ref::At(100, 1),
    ));
    s.extend(shares(
        "leading corner",
        &[(2, 3), (2, 4), (2, 5), (3, 2)],
        Ref::At(2, 0),
        Ref::At(2, 12),
    ));
    s.extend(shares(
        "top-trailing corner",
        &[(193, 1), (194, 1), (195, 1), (195, 2), (196, 2)],
        Ref::At(199, 1),
        Ref::At(100, 1),
    ));
    s.extend(shares(
        "bottom-leading corner",
        &[(4, 22), (5, 22), (6, 22)],
        Ref::At(0, 22),
        Ref::At(100, 22),
    ));
    s
}

/// A selected row: the highlight, filled and outlined in it.
fn selected_row_samples() -> Vec<Sample> {
    let mut s = selection_frame();
    s.extend(solids(
        "selection, outline and fill",
        &[(100, 12), (2, 12), (197, 12), (100, 1), (100, 22)],
    ));
    s
}

/// A hovered row: the wash, and the outline in the highlight at 80 % over
/// it.
fn hovered_row_samples() -> Vec<Sample> {
    let mut s = selection_frame();
    s.push(solid("wash", 100, 12));
    s.extend(shares(
        "outline at 80 %",
        &[(2, 12), (197, 12), (100, 1), (100, 22)],
        Ref::At(100, 0),
        Ref::Role(PaletteRole::Highlight),
    ));
    s
}

/// An unhovered, unselected row: nothing.
fn plain_row_samples() -> Vec<Sample> {
    solids(
        "nothing painted",
        &[(100, 12), (2, 12), (100, 1), (3, 2), (2, 3)],
    )
}

fn row(
    id: &'static str,
    mount: fn(&mut WidgetTree) -> WidgetId,
    prepare: fn(&mut WidgetTree, WidgetId),
    samples: Vec<Sample>,
) -> Scenario {
    Scenario {
        id,
        canvas: (200, 24),
        origin: (0, 0),
        mount,
        prepare,
        samples,
    }
}

/// A list row selected, hovered and at rest: the selection's rectangle, its
/// corners and its outline.
#[test]
fn list_rows_land_on_plasmas_pixels() {
    assert_agree(
        "list_rows_land_on_plasmas_pixels",
        vec![
            row(
                "item_view_row.selected",
                selected_row,
                at_rest,
                selected_row_samples(),
            ),
            row(
                "item_view_row.hover",
                plain_row,
                hovered,
                hovered_row_samples(),
            ),
            row(
                "item_view_row.normal",
                plain_row,
                at_rest,
                plain_row_samples(),
            ),
        ],
    );
}

// ── The scroll bar ──────────────────────────────────────────────────────

fn scroll_bar(tree: &mut WidgetTree) -> WidgetId {
    // The handle Plasma paints: a page of 20 in a range of 0 to 100, at the
    // top, 32 px long on its 194 px groove; 0.16 of the 200 px bar here.
    tree.add(
        ScrollBar::new(
            ScrollBarOrientation::Vertical,
            Signal::new(0.0),
            Signal::new(100.0),
            Signal::new(0.16),
        )
        .thickness(21.0)
        .variant(ScrollBarVariant::Permanent),
    )
}

/// Breeze starts the handle 3 px down the bar (see the module
/// documentation).
const HANDLE_SHIFT: (isize, isize) = (0, -3);

fn handle(what: &'static str, x: usize, y: usize, check: Check) -> Sample {
    Sample {
        what,
        at: (x, y),
        check,
        shift: HANDLE_SHIFT,
    }
}

/// The scroll bar: the 1 px separator on its leading edge in the view's
/// text at the frame contrast, the whole length of the bar; the handle 8 px
/// wide, 6 px from the separator, covering the pixels Plasma's covers
/// across the bar and along it, and nothing on the track.
fn scroll_bar_samples() -> Vec<Sample> {
    let mut s = shares(
        "separator",
        &[(0, 18), (0, 100), (0, 150), (0, 199)],
        Ref::At(1, 150),
        Ref::Role(PaletteRole::Text),
    );
    s.extend(solids(
        "no track",
        &[(1, 150), (10, 150), (20, 150), (10, 60)],
    ));
    for x in [1, 6, 15, 20] {
        s.push(handle("beside the handle", x, 18, Check::Bare));
    }
    for x in 7..=14 {
        s.push(handle("across the handle", x, 18, Check::Ink));
    }
    for y in [3, 4, 5, 18, 32, 33, 34] {
        s.push(handle("along the handle", 10, y, Check::Ink));
    }
    for y in [35, 36, 40] {
        s.push(handle("past the handle's end", 10, y, Check::Bare));
    }
    s
}

/// The scroll bar at rest.
#[test]
fn the_scroll_bar_lands_on_plasmas_pixels() {
    assert_agree(
        "the_scroll_bar_lands_on_plasmas_pixels",
        vec![Scenario {
            id: "scroll_bar.normal",
            canvas: (21, 200),
            origin: (0, 0),
            mount: scroll_bar,
            prepare: at_rest,
            samples: scroll_bar_samples(),
        }],
    );
}

// ── The comparison itself ───────────────────────────────────────────────

/// The share is the coverage in each space: a colour 30 % of the way from
/// one colour to another in sRGB is 0.3 there, and the same 30 % composited
/// in linear light is 0.3 in linear light.
#[test]
fn a_share_is_measured_in_each_images_compositing_space() {
    let window = [239, 240, 241, 255];
    let black = [0, 0, 0, 255];
    // Black at 12.5 % over the window, composited in sRGB, as Qt does.
    let qt = [209, 210, 211, 255];
    let (in_srgb, step) = share_of(qt, window, black, false).unwrap();
    assert!((in_srgb - 0.125).abs() < 0.01, "{in_srgb}");
    assert!(step <= MAX_SHARE_STEP);
    // The same composited in linear light, as the renderer does: a lighter
    // colour, the same share.
    let linear = |v: u8| teksilo_render::vertex::srgb_to_linear_rgba([f32::from(v) / 255.0; 4])[0];
    let encode = |l: f32| {
        let e = if l <= 0.003_130_8 {
            l * 12.92
        } else {
            1.055 * l.powf(1.0 / 2.4) - 0.055
        };
        (e * 255.0).round() as u8
    };
    let gpu = window.map(|v| encode(linear(v) * 0.875));
    let gpu = [gpu[0], gpu[1], gpu[2], 255];
    assert!(gpu[0] > qt[0] + 10, "{gpu:?}: lighter");
    let (in_linear, _) = share_of(gpu, window, black, true).unwrap();
    assert!((in_linear - 0.125).abs() < 0.01, "{in_linear}");
    // Read in the wrong space it would be far off: why each is read in its
    // own.
    assert!(share_of(gpu, window, black, false).unwrap().0 < 0.08);
    // Near black one level moves the linear share too far to measure it.
    let dark = [16, 17, 18, 255];
    assert!(share_of([14, 15, 16, 255], dark, black, true).unwrap().1 > MAX_SHARE_STEP);
    assert_eq!(share_of(qt, window, window, false), None);
}

/// The signed distance from `(px, py)` to the rounded rectangle `rect` of
/// radius `r`: negative inside.
fn rounded_rect_distance(px: f32, py: f32, rect: [f32; 4], r: f32) -> f32 {
    let [x, y, w, h] = rect;
    let qx = (px - (x + w / 2.0)).abs() - w / 2.0 + r;
    let qy = (py - (y + h / 2.0)).abs() - h / 2.0 + r;
    qx.max(qy).min(0.0) + qx.max(0.0).hypot(qy.max(0.0)) - r
}

/// The share of the pixel `(x, y)` a 1 px outline stroked on `rect` at
/// radius `r` covers, by area, on a 64 x 64 grid of subsamples.
fn outline_area(x: usize, y: usize, rect: [f32; 4], r: f32) -> f32 {
    const N: usize = 64;
    let mut inside = 0;
    for i in 0..N {
        for j in 0..N {
            let px = x as f32 + (i as f32 + 0.5) / N as f32;
            let py = y as f32 + (j as f32 + 0.5) / N as f32;
            if rounded_rect_distance(px, py, rect, r).abs() <= 0.5 {
                inside += 1;
            }
        }
    }
    inside as f32 / (N * N) as f32
}

/// [`RENDERER_COVERAGE_GAP`] is what it says: the smoothstep's largest
/// departure from a linear ramp, which is an edge's area coverage, across
/// the one pixel the shape pipeline ramps over.
#[test]
fn the_renderer_coverage_gap_is_the_smoothsteps() {
    let gap = (0..=10_000)
        .map(|i| {
            let t = i as f32 / 10_000.0;
            (t * t * (3.0 - 2.0 * t) - t).abs()
        })
        .fold(0.0, f32::max);
    assert!(
        gap <= RENDERER_COVERAGE_GAP && gap > RENDERER_COVERAGE_GAP - 0.002,
        "{gap}"
    );
    // The arcs sampled here depart no further: the push button's outline at
    // 4.5, the row's at 5 and the box's at 4, through the renderer's own
    // ramp.
    for (rect, r) in [
        ([1.5, 1.5, 81.0, 31.0], 4.5),
        ([2.5, 1.5, 195.0, 21.0], 5.0),
        ([2.5, 2.5, 15.0, 15.0], 4.0),
    ] {
        for x in 0..9 {
            for y in 0..9 {
                let d = rounded_rect_distance(x as f32 + 0.5, y as f32 + 0.5, rect, r).abs();
                let t = d.clamp(0.0, 1.0);
                let ramp = 1.0 - t * t * (3.0 - 2.0 * t);
                let area = outline_area(x, y, rect, r);
                assert!(
                    (ramp - area).abs() <= RENDERER_COVERAGE_GAP,
                    "{rect:?} at ({x}, {y}): {ramp} against {area}"
                );
            }
        }
    }
}

/// [`QT_CORNER_GAP`] is measured, not chosen: the largest departure of
/// Plasma's push-button corner pixels from the area of a 1 px outline at
/// radius 4.5 on the rectangle the specification gives, over every button
/// sampled, in both schemes.
#[test]
fn qts_corners_depart_from_the_specified_shape_by_the_qt_gap() {
    let rect = [1.5, 1.5, 81.0, 31.0];
    let mut largest: f32 = 0.0;
    for scheme in ["BreezeLight", "BreezeDark"] {
        for id in [
            "push_button.normal",
            "push_button.hover",
            "push_button.pressed",
            "push_button.default",
        ] {
            let g = Golden::load(scheme, id);
            for ((x, y), under, over) in [
                ((3, 1), (0, 1), (42, 1)),
                ((4, 1), (0, 1), (42, 1)),
                ((5, 1), (0, 1), (42, 1)),
                ((1, 3), (1, 0), (1, 17)),
                ((1, 4), (1, 0), (1, 17)),
                ((1, 5), (1, 0), (1, 17)),
                ((2, 2), (0, 2), (42, 1)),
            ] {
                let (qt, _) = share_of(
                    g.at(x, y),
                    g.at(under.0, under.1),
                    g.at(over.0, over.1),
                    false,
                )
                .expect("distinct colours");
                largest = largest.max((qt - outline_area(x, y, rect, 4.5)).abs());
            }
        }
    }
    assert!(
        largest <= QT_CORNER_GAP && largest > QT_CORNER_GAP - 0.01,
        "{largest}"
    );
}
