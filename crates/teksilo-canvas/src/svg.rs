// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! SVG parsing: load SVG icons as [`Path`] geometry for rendering.
//!
//! The main entry point is [`SvgIcon::parse`], which takes an SVG string and
//! produces geometry in viewBox coordinates. Both *filled* and *stroked*
//! (line-style) icons are supported. The parser tracks each element's paint
//! state — `fill` / `stroke` / `stroke-width` / `stroke-linecap` /
//! `stroke-linejoin` / `stroke-miterlimit` / `fill-rule` / `opacity` /
//! `fill-opacity` / `stroke-opacity` / `stroke-dasharray` / `display` /
//! `visibility` — from presentation attributes, `<style>` CSS rules (tag /
//! `.class` / `#id` selectors), and the inline `style` attribute, inheriting
//! through `<g>` groups and the `<svg>` root the way SVG does. So the line-style
//! convention (`fill="none" stroke="currentColor"`, however it's applied)
//! renders as outlines instead of solid blobs.
//!
//! # Two renderings of the same document
//!
//! An icon is parsed once into **two** representations, because the two things
//! apps do with vector art want different guarantees:
//!
//! * **Tinted** — [`raw_path`](SvgIcon::raw_path) +
//!   [`extra_fills`](SvgIcon::extra_fills) + [`strokes`](SvgIcon::strokes).
//!   Colors are discarded and the widget supplies one; every compatible shape is
//!   merged into a single [`Path`], so a themed UI glyph is one fill call and one
//!   atlas tile, and it follows light/dark for free. This is what an icon *font*
//!   would give you, and it is the right default for UI chrome.
//!
//! * **Full-color** — [`draw_ops_in_rect`](SvgIcon::draw_ops_in_rect), backed by
//!   [`SvgOp`]. Each shape keeps the paint it was authored with — a color, or a
//!   gradient ([`ResolvedGradient`]) — and the ops stay in **document order**,
//!   because with more than one color the overlap order is suddenly load-bearing:
//!   the merged representation paints all fills and then all strokes, which is
//!   invisible in one tint and wrong the moment a dark shape is supposed to sit
//!   *behind* a light one. `currentColor` still resolves to the widget's tint, so
//!   artwork can mix fixed brand colors with one themed accent.
//!
//! Both are built in a single walk. An icon that declares no color of its own
//! reports [`is_monochrome`](SvgIcon::is_monochrome), and renders identically
//! either way.
//!
//! `<use>` references resolve against an id index (including `<symbol>`); the
//! non-rendering containers (`<defs>` / `<symbol>` / `<clipPath>` / `<mask>` /
//! `<pattern>` / `<marker>`) don't paint directly; `<linearGradient>` /
//! `<radialGradient>` are collected as paint servers (with `href` inheritance)
//! and bound to whichever shapes reference them; `preserveAspectRatio` controls
//! the fit.
//!
//! **Out of scope (known limitations):** `<text>` / `<tspan>` / `<image>` and
//! `<switch>` are skipped — an SVG that is just a wrapper around an embedded
//! raster therefore renders as nothing, by design: it is not vector art.
//! `<pattern>` is parsed as a container but not implemented as a paint server (a
//! shape referencing one falls back to the tint rather than disappearing, as does
//! any dangling `url(#…)`). Gradients are limited by what the renderer's gradient
//! pipeline can express: at most **4 stops** (a 5th is dropped — a framework-wide
//! limit, not an SVG one), `spreadMethod` is always `pad`, a radial gradient's
//! focal point (`fx`/`fy`) is ignored, and an `objectBoundingBox` radial on a
//! non-square box is drawn as the equal-area circle instead of SVG's ellipse.
//! Group `opacity` is approximated as per-element alpha (overlaps double-tint);
//! in the *tinted* representation paint order is global (all fills, then strokes)
//! rather than document order — the full-color one keeps document order;
//! `vector-effect="non-scaling-stroke"` is ignored; and CSS is limited to simple
//! selectors (no combinators / pseudo-classes / `@media`).

mod color;
mod gradient;
pub(crate) mod path_parser;

use std::collections::HashMap;

use teksilo_tokens::Color;

use crate::geometry::{Point, Rect, Transform2D};
use crate::paint::{FillRule, GradientStop, LineCap, LineJoin, Paint, StrokeStyle};
use crate::path::Path;
use crate::xml::{XmlElement, parse_dom};

use color::parse_color;
use gradient::{GradientDef, collect_gradients};

pub use gradient::{ResolvedGradient, SvgStop};

/// Error type for SVG parsing failures.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SvgParseError {
    /// XML is malformed.
    #[error("SVG XML error: {0}")]
    XmlError(String),
    /// No `<svg>` root element found.
    #[error("no <svg> root element found")]
    MissingSvgElement,
    /// viewBox attribute is missing or invalid.
    #[error("invalid viewBox: {0}")]
    InvalidViewBox(String),
    /// Path data string (`d` attribute) is malformed.
    #[error("invalid path data at position {position}: {detail}")]
    InvalidPathData { detail: String, position: usize },
    /// A `transform` attribute could not be parsed.
    #[error("invalid transform: {0}")]
    InvalidTransform(String),
}

/// One stroked sub-path of an SVG icon: outline geometry plus the
/// stroke width in viewBox units and the cap/join style. Produced for
/// elements painted with `stroke` (the "line-style" icon convention,
/// usually paired with `fill="none"`). Colors are stripped — the
/// rendering widget supplies the tint.
///
/// The `width` is in **viewBox** coordinates; it scales together with
/// the geometry when the icon is fitted to a display rect (see
/// [`SvgIcon::stroked_paths_in_rect`]).
#[derive(Debug, Clone)]
pub struct SvgStroke {
    /// Outline geometry in viewBox coordinates. May contain several
    /// sub-contours (each `MoveTo` starts a new one); the renderer
    /// strokes each contour independently.
    pub path: Path,
    /// Stroke width in viewBox units.
    pub width: f32,
    /// Line cap for open contours.
    pub line_cap: LineCap,
    /// Line join at contour vertices.
    pub line_join: LineJoin,
    /// Miter limit (`stroke-miterlimit`); 4.0 by default.
    pub miter_limit: f32,
    /// Opacity (`stroke-opacity` × ancestor `opacity`), in `[0, 1]`.
    pub opacity: f32,
    /// `stroke-dasharray` (viewBox-space lengths) + `stroke-dashoffset`,
    /// if dashed.
    pub dash: Option<(Vec<f32>, f32)>,
}

/// One non-default *filled* sub-path of an SVG icon — either even-odd or
/// partially transparent (a plain winding, fully-opaque fill merges into
/// [`SvgIcon::raw_path`] instead). Colors are stripped; the widget tints.
#[derive(Debug, Clone)]
pub struct SvgFill {
    /// Fill geometry in viewBox coordinates.
    pub path: Path,
    /// Even-odd vs non-zero winding.
    pub fill_rule: FillRule,
    /// Opacity (`fill-opacity` × ancestor `opacity`), in `[0, 1]`.
    pub opacity: f32,
}

/// A parsed SVG icon: geometry + viewBox, ready to be scaled and
/// rendered. Original colors are stripped; filled and stroked geometry
/// are kept separately so line-style icons render as outlines.
#[derive(Debug, Clone)]
pub struct SvgIcon {
    /// Merged *default* fill (non-zero winding, fully opaque) in viewBox
    /// coordinates — the hot path, returned by [`raw_path`](Self::raw_path).
    path: Path,
    /// Non-default fills (even-odd or `opacity < 1`), each carrying its
    /// own rule + opacity. Empty for the common icon.
    extra_fills: Vec<SvgFill>,
    /// *Stroked* sub-paths in viewBox coordinates, grouped by stroke
    /// style. Empty for the common all-filled icon.
    strokes: Vec<SvgStroke>,
    /// The same shapes again, but unmerged, in document order, each carrying
    /// the color it was authored with — the full-color representation. See
    /// [`SvgOp`] for why the merged one above can't serve both.
    ops: Vec<SvgOp>,
    /// The SVG viewBox (defines the coordinate space).
    view_box: Rect,
    /// How the viewBox maps into a target rect (`preserveAspectRatio`).
    aspect: AspectRatio,
}

/// Alignment of the scaled viewBox within the target rect along one axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Align {
    Min,
    Mid,
    Max,
}

/// SVG `preserveAspectRatio` — how the viewBox is scaled and aligned into
/// a target rect. Default is `xMidYMid meet` (uniform fit, centered).
#[derive(Debug, Clone, Copy, PartialEq)]
struct AspectRatio {
    x: Align,
    y: Align,
    /// `meet` (fit, default) vs `slice` (cover).
    slice: bool,
    /// `none` — non-uniform stretch to fill (ignores `x`/`y`/`slice`).
    stretch: bool,
}

impl Default for AspectRatio {
    fn default() -> Self {
        Self {
            x: Align::Mid,
            y: Align::Mid,
            slice: false,
            stretch: false,
        }
    }
}

impl SvgIcon {
    /// Parse an SVG string into an `SvgIcon`.
    pub fn parse(svg_str: &str) -> Result<Self, SvgParseError> {
        let root = parse_dom(svg_str)
            .map_err(SvgParseError::XmlError)?
            .ok_or(SvgParseError::MissingSvgElement)?;

        // The document root must be (or contain) an <svg> element.
        let svg_el = if root.tag_name() == "svg" {
            &root
        } else {
            root.children()
                .find(|n| n.tag_name() == "svg")
                .ok_or(SvgParseError::MissingSvgElement)?
        };

        let view_box = parse_view_box(svg_el)?;

        // Index every element carrying an `id` so `<use href="#id">` can
        // resolve references (including forward references).
        let mut id_map = HashMap::new();
        build_id_map(svg_el, &mut id_map);
        let css = collect_style_rules(svg_el);
        // Paint servers are collected up front, from the id index rather than
        // the walk: they live in `<defs>` (which the walk skips, correctly —
        // definitions don't paint), and a shape may reference one declared
        // *after* it in document order.
        let gradients = collect_gradients(&id_map, view_box);
        let ctx = WalkCtx {
            id_map: &id_map,
            css: &css,
            gradients: &gradients,
        };

        let mut builder = SvgBuilder::default();
        walk_element(
            svg_el,
            &Transform2D::IDENTITY,
            SvgPaintState::default(),
            &mut builder,
            &ctx,
            0,
        )?;

        let aspect = svg_el
            .attribute("preserveAspectRatio")
            .map(parse_preserve_aspect_ratio)
            .unwrap_or_default();

        Ok(SvgIcon {
            path: builder.fill,
            extra_fills: builder.extra_fills,
            strokes: builder.strokes,
            ops: builder.ops,
            view_box,
            aspect,
        })
    }

    /// The viewBox width (natural width of the icon).
    pub fn width(&self) -> f32 {
        self.view_box.width
    }

    /// The viewBox height (natural height of the icon).
    pub fn height(&self) -> f32 {
        self.view_box.height
    }

    /// Produce a [`Path`] scaled to fit within a square of the given size,
    /// preserving aspect ratio and centering.
    pub fn to_path(&self, size: f32) -> Path {
        self.to_path_in_rect(Rect::new(0.0, 0.0, size, size))
    }

    /// Produce the *filled* [`Path`] scaled to fit within `rect`,
    /// preserving aspect ratio and centering. Empty for a pure
    /// line-style icon (use [`stroked_paths_in_rect`](Self::stroked_paths_in_rect)
    /// for its outlines).
    pub fn to_path_in_rect(&self, rect: Rect) -> Path {
        if self.path.is_empty() {
            return Path::new();
        }
        match self.fit_transform(rect) {
            Some((transform, _)) => self.path.transformed(&transform),
            None => Path::new(),
        }
    }

    /// Produce the *stroked* sub-paths scaled to fit within `rect`, each
    /// as `(path, style, opacity)` — a ready-to-render [`StrokeStyle`]
    /// whose width / dash are scaled into display space, plus the stroke
    /// opacity in `[0, 1]` (multiply it into the tint's alpha). Empty for
    /// the common filled-only icon.
    ///
    /// Pair with [`to_path_in_rect`](Self::to_path_in_rect): fill the
    /// returned path, then stroke each of these — an icon may carry both
    /// (a filled shape with a stroked border).
    pub fn stroked_paths_in_rect(&self, rect: Rect) -> Vec<(Path, StrokeStyle, f32)> {
        if self.strokes.is_empty() {
            return Vec::new();
        }
        let Some((transform, scale)) = self.fit_transform(rect) else {
            return Vec::new();
        };
        self.strokes
            .iter()
            .map(|s| {
                let mut style = StrokeStyle::solid(s.width * scale);
                style.line_cap = s.line_cap;
                style.line_join = s.line_join;
                style.miter_limit = s.miter_limit;
                if let Some((arr, off)) = &s.dash {
                    style.dash_pattern = Some(arr.iter().map(|d| d * scale).collect());
                    style.dash_offset = off * scale;
                }
                (s.path.transformed(&transform), style, s.opacity)
            })
            .collect()
    }

    /// Produce the *non-default* fills (even-odd or partially transparent)
    /// scaled to fit within `rect`, each paired with its [`FillRule`] and
    /// opacity. Empty for the common icon (whose fills all merge into
    /// [`to_path_in_rect`](Self::to_path_in_rect)).
    pub fn extra_fills_in_rect(&self, rect: Rect) -> Vec<(Path, FillRule, f32)> {
        if self.extra_fills.is_empty() {
            return Vec::new();
        }
        let Some((transform, _)) = self.fit_transform(rect) else {
            return Vec::new();
        };
        self.extra_fills
            .iter()
            .map(|f| (f.path.transformed(&transform), f.fill_rule, f.opacity))
            .collect()
    }

    /// Fit this icon into `rect` **in its own colors**, as an ordered list of
    /// canvas-ready draw operations — the full-color counterpart of
    /// [`to_path_in_rect`](Self::to_path_in_rect) + friends, which paint the
    /// same shapes as a single tinted silhouette.
    ///
    /// `current` is the widget's color, and it does two jobs (the same two
    /// [`IconMode::FullColor`] gives it for a raster):
    /// * it *is* the color of anything authored `currentColor` — the SVG
    ///   convention for "let the host decide", and the escape hatch for an icon
    ///   that wants a themed accent inside otherwise fixed artwork; and
    /// * its alpha attenuates the whole icon, so a disabled or fading icon
    ///   dims without its colors being replaced.
    ///
    /// Ops come back in document order — later ones paint over earlier ones, as
    /// SVG requires. Empty if the viewBox is degenerate.
    ///
    /// [`IconMode::FullColor`]: https://docs.rs/teksilo-widgets/latest/teksilo_widgets/primitives/icon_widget/enum.IconMode.html
    pub fn draw_ops_in_rect(&self, rect: Rect, current: Color) -> Vec<SvgDrawOp> {
        if self.ops.is_empty() {
            return Vec::new();
        }
        let Some((transform, scale)) = self.fit_transform(rect) else {
            return Vec::new();
        };

        let mut out = Vec::with_capacity(self.ops.len());
        for op in &self.ops {
            let path = op.path.transformed(&transform);
            if path.is_empty() {
                continue;
            }
            // Gradient geometry is expressed relative to the path's own bounds
            // (the space the canvas normalizes a path gradient against), so it
            // has to be re-based per op, after the fit.
            let paint =
                resolve_draw_paint(&op.paint, current, op.opacity, &transform, scale, &path);
            if paint_is_invisible(&paint) {
                continue;
            }
            out.push(match &op.kind {
                SvgOpKind::Fill { fill_rule } => SvgDrawOp::Fill {
                    path,
                    fill_rule: *fill_rule,
                    paint,
                },
                SvgOpKind::Stroke {
                    width,
                    line_cap,
                    line_join,
                    miter_limit,
                    dash,
                } => {
                    let mut style = StrokeStyle::solid(width * scale);
                    style.line_cap = *line_cap;
                    style.line_join = *line_join;
                    style.miter_limit = *miter_limit;
                    if let Some((arr, off)) = dash {
                        style.dash_pattern = Some(arr.iter().map(|d| d * scale).collect());
                        style.dash_offset = off * scale;
                    }
                    SvgDrawOp::Stroke { path, style, paint }
                }
            });
        }
        out
    }

    /// Whether the icon carries no color of its own — every painted shape is
    /// `currentColor` (or an unresolvable paint). Such an icon renders
    /// identically in either [`IconMode`], so a widget can skip the ordered
    /// full-color walk and take the merged tinted fast path.
    ///
    /// [`IconMode`]: https://docs.rs/teksilo-widgets/latest/teksilo_widgets/primitives/icon_widget/enum.IconMode.html
    pub fn is_monochrome(&self) -> bool {
        self.ops
            .iter()
            .all(|op| matches!(op.paint, SvgPaint::Current))
    }

    /// The ordered, color-carrying draw ops in viewBox coordinates.
    pub fn ops(&self) -> &[SvgOp] {
        &self.ops
    }

    /// The `preserveAspectRatio` fit transform mapping viewBox coordinates
    /// into `rect`, together with a representative scale factor (used to
    /// scale stroke widths). `None` if the viewBox is degenerate.
    fn fit_transform(&self, rect: Rect) -> Option<(Transform2D, f32)> {
        let vb = self.view_box;
        if vb.width <= 0.0 || vb.height <= 0.0 {
            return None;
        }
        let sx = rect.width / vb.width;
        let sy = rect.height / vb.height;

        // `none` — non-uniform stretch to fill. The representative scale for
        // stroke widths is the geometric mean of the two axis scales.
        if self.aspect.stretch {
            let transform = Transform2D::scale(sx, sy).then(&Transform2D::translate(
                rect.x - vb.x * sx,
                rect.y - vb.y * sy,
            ));
            return Some((transform, (sx * sy).sqrt()));
        }

        // Uniform `meet` (fit) or `slice` (cover), then align the leftover.
        let scale = if self.aspect.slice {
            sx.max(sy)
        } else {
            sx.min(sy)
        };
        let align = |leftover: f32, a: Align| match a {
            Align::Min => 0.0,
            Align::Mid => leftover / 2.0,
            Align::Max => leftover,
        };
        let offset_x = rect.x + align(rect.width - vb.width * scale, self.aspect.x);
        let offset_y = rect.y + align(rect.height - vb.height * scale, self.aspect.y);

        let transform = Transform2D::scale(scale, scale).then(&Transform2D::translate(
            offset_x - vb.x * scale,
            offset_y - vb.y * scale,
        ));
        Some((transform, scale))
    }

    /// Access the raw *filled* path in viewBox coordinates.
    pub fn raw_path(&self) -> &Path {
        &self.path
    }

    /// Access the *stroked* sub-paths in viewBox coordinates.
    pub fn strokes(&self) -> &[SvgStroke] {
        &self.strokes
    }

    /// Access the *non-default* fills (even-odd / transparent) in viewBox
    /// coordinates.
    pub fn extra_fills(&self) -> &[SvgFill] {
        &self.extra_fills
    }

    /// Whether this icon carries any geometry at all (filled or stroked).
    pub fn is_empty(&self) -> bool {
        self.path.is_empty() && self.extra_fills.is_empty() && self.strokes.is_empty()
    }

    /// Access the viewBox.
    pub fn view_box(&self) -> Rect {
        self.view_box
    }
}

// --- Internal helpers ---

/// Apply the alpha model to one color of a full-color icon.
///
/// `base` is the authored color, or `None` for `currentColor`. `op_alpha` is the
/// element's `opacity` × `fill-opacity`/`stroke-opacity`, and `tint` is the
/// widget's color.
///
/// The tint's alpha counts **once**, never twice: an authored color is
/// attenuated by it, and a `currentColor` *is* the tint and so already carries
/// it. Multiplying in both places is the easy mistake here, and it makes a
/// half-transparent icon render at quarter opacity.
fn tinted(base: Option<Color>, tint: Color, op_alpha: f32) -> Color {
    match base {
        Some(c) => c.with_alpha(c.a() * op_alpha * tint.a()),
        None => tint.with_alpha(tint.a() * op_alpha),
    }
}

/// Resolve one op's paint into a canvas [`Paint`], in the coordinate space the
/// canvas expects: gradient geometry relative to `path`'s own bounds.
fn resolve_draw_paint(
    paint: &SvgPaint,
    tint: Color,
    op_alpha: f32,
    transform: &Transform2D,
    scale: f32,
    path: &Path,
) -> Paint {
    match paint {
        SvgPaint::Current => Paint::Solid(tinted(None, tint, op_alpha)),
        SvgPaint::Solid(c) => Paint::Solid(tinted(Some(*c), tint, op_alpha)),
        SvgPaint::Gradient(g) => {
            let b = path.bounds();
            // viewBox → display, then display → path-bounds-local.
            let local = |p: Point| {
                let d = transform.apply_point(p);
                Point::new(d.x - b.x, d.y - b.y)
            };
            let ramp = |stops: &[SvgStop]| -> Vec<GradientStop> {
                stops
                    .iter()
                    .map(|s| GradientStop {
                        offset: s.offset,
                        color: tinted(s.color, tint, op_alpha * s.opacity),
                    })
                    .collect()
            };
            match g {
                ResolvedGradient::Linear { start, end, stops } => Paint::LinearGradient {
                    start: local(*start),
                    end: local(*end),
                    stops: ramp(stops),
                },
                ResolvedGradient::Radial {
                    center,
                    radius,
                    stops,
                } => Paint::RadialGradient {
                    center: local(*center),
                    radius: radius * scale,
                    stops: ramp(stops),
                },
            }
        }
    }
}

/// Whether a resolved paint would draw nothing at all — a fully transparent
/// solid, or a ramp whose every stop is transparent. Skipping these keeps a
/// `fill-opacity="0"` shape (a common way to author an invisible hit target)
/// from costing a path rasterization.
fn paint_is_invisible(paint: &Paint) -> bool {
    match paint {
        Paint::Solid(c) => c.a() <= 0.0,
        Paint::LinearGradient { stops, .. }
        | Paint::RadialGradient { stops, .. }
        | Paint::ConicGradient { stops, .. } => stops.iter().all(|s| s.color.a() <= 0.0),
        Paint::Image(_) => false,
    }
}

fn parse_view_box(svg_el: &XmlElement) -> Result<Rect, SvgParseError> {
    if let Some(vb) = svg_el.attribute("viewBox") {
        let nums: Vec<f32> = vb
            .split(|c: char| c.is_ascii_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.parse::<f32>()
                    .map_err(|_| SvgParseError::InvalidViewBox(format!("invalid number '{s}'")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if nums.len() != 4 {
            return Err(SvgParseError::InvalidViewBox(format!(
                "expected 4 values, got {}",
                nums.len()
            )));
        }
        return Ok(Rect::new(nums[0], nums[1], nums[2], nums[3]));
    }
    // Fallback to width/height attributes
    let w = svg_el
        .attribute("width")
        .and_then(parse_length)
        .ok_or_else(|| SvgParseError::InvalidViewBox("no viewBox or width attribute".into()))?;
    let h = svg_el
        .attribute("height")
        .and_then(parse_length)
        .ok_or_else(|| SvgParseError::InvalidViewBox("no viewBox or height attribute".into()))?;
    Ok(Rect::new(0.0, 0.0, w, h))
}

/// Parse a length value. Absolute unit suffixes (`px`/`pt`/`mm`/`cm`/`in`)
/// are stripped and the number kept (SVG's user unit is px-equivalent).
/// Percentages and font-relative units (`%`/`em`/`ex`/`rem`) can't be
/// resolved without a viewport / font, so they return `None` (the caller
/// falls back) rather than misparsing — e.g. `width="100%"` with no
/// viewBox degrades to the viewBox error instead of a confusing one.
fn parse_length(s: &str) -> Option<f32> {
    let s = s.trim();
    if s.ends_with('%') || s.ends_with("em") || s.ends_with("ex") {
        return None;
    }
    let num = s
        .trim_end_matches("px")
        .trim_end_matches("pt")
        .trim_end_matches("mm")
        .trim_end_matches("cm")
        .trim_end_matches("in")
        .trim();
    num.parse::<f32>().ok()
}

/// Approximate equality for grouping float paint parameters.
const GROUP_EPS: f32 = 1e-4;

/// Accumulates parsed geometry in both representations at once: the merged /
/// grouped one the tinted fast path draws (the default fill path, non-default
/// fills, stroked sub-paths), and the document-ordered, color-carrying [`SvgOp`]
/// list full-color rendering walks. One traversal feeds both — they are two
/// views of the same shapes, so deriving one from the other later would just be
/// the same work done twice.
#[derive(Default)]
struct SvgBuilder {
    fill: Path,
    extra_fills: Vec<SvgFill>,
    strokes: Vec<SvgStroke>,
    ops: Vec<SvgOp>,
}

impl SvgBuilder {
    /// Record one draw op, preserving document order (the whole point of the
    /// op list — see [`SvgOp`]).
    fn push_op(&mut self, op: SvgOp) {
        self.ops.push(op);
    }

    /// Add a filled sub-path. A non-zero winding, fully-opaque fill (the
    /// overwhelmingly common case) merges into the single `fill` path;
    /// even-odd or partially-transparent fills go to `extra_fills`,
    /// grouped by `(rule, opacity)` so an icon authored uniformly stays
    /// one entry.
    fn push_fill(&mut self, path: Path, fill_rule: FillRule, opacity: f32) {
        if fill_rule == FillRule::Winding && (opacity - 1.0).abs() < GROUP_EPS {
            self.fill.append(&path);
            return;
        }
        if let Some(group) = self
            .extra_fills
            .iter_mut()
            .find(|f| f.fill_rule == fill_rule && (f.opacity - opacity).abs() < GROUP_EPS)
        {
            group.path.append(&path);
        } else {
            self.extra_fills.push(SvgFill {
                path,
                fill_rule,
                opacity,
            });
        }
    }

    /// Add a stroked sub-path, merging it into an existing group that
    /// shares the same width / cap / join / opacity / dash so the common
    /// "one stroke style for the whole icon" case stays a single entry
    /// (and one rasterized atlas tile). Strokes are per-contour, so
    /// appending extra `MoveTo`-started contours is equivalent to
    /// stroking each separately.
    #[allow(clippy::too_many_arguments)] // stroke paint params; bundling adds no clarity
    fn push_stroke(
        &mut self,
        path: Path,
        width: f32,
        line_cap: LineCap,
        line_join: LineJoin,
        miter_limit: f32,
        opacity: f32,
        dash: Option<(Vec<f32>, f32)>,
    ) {
        if let Some(group) = self.strokes.iter_mut().find(|s| {
            (s.width - width).abs() < GROUP_EPS
                && s.line_cap == line_cap
                && s.line_join == line_join
                && (s.miter_limit - miter_limit).abs() < GROUP_EPS
                && (s.opacity - opacity).abs() < GROUP_EPS
                && s.dash == dash
        }) {
            group.path.append(&path);
        } else {
            self.strokes.push(SvgStroke {
                path,
                width,
                line_cap,
                line_join,
                miter_limit,
                opacity,
                dash,
            });
        }
    }
}

/// A `fill` / `stroke` value as *declared*: still a reference, not yet bound to
/// the shape it paints.
///
/// A paint server (`url(#grad)`) can't be resolved during the cascade, because
/// SVG's default `gradientUnits="objectBoundingBox"` makes the answer depend on
/// the *shape* — the same declaration lands differently on every element that
/// inherits it. So the cascade carries the reference and the walk resolves it
/// per shape, once it knows the geometry (see [`resolve_paint`]).
#[derive(Debug, Clone, PartialEq)]
enum PaintRef {
    /// `none` — the shape is not painted on this channel at all.
    None,
    /// `currentColor` — defers to whatever tint the widget supplies.
    Current,
    /// A literal color.
    Color(Color),
    /// `url(#id)`, plus the optional fallback color CSS allows after it
    /// (`fill="url(#grad) red"`) for when the reference doesn't resolve.
    Ref { id: String, fallback: Option<Color> },
}

impl PaintRef {
    /// Whether this channel paints anything at all.
    fn is_painted(&self) -> bool {
        !matches!(self, PaintRef::None)
    }
}

/// The paint of one [`SvgOp`], resolved into the icon's viewBox space.
#[derive(Debug, Clone, PartialEq)]
pub enum SvgPaint {
    /// `currentColor` — and also the fallback for any paint the parser can't
    /// resolve (a `url(#…)` naming a missing id, or a paint server this parser
    /// doesn't implement, e.g. `<pattern>`). Both defer to the widget's tint,
    /// which keeps an unsupported paint *visible* rather than dropping the
    /// shape.
    Current,
    /// A literal color, alpha included.
    Solid(Color),
    /// A gradient, already bound to the shape it paints.
    Gradient(ResolvedGradient),
}

/// How one [`SvgOp`] paints its geometry.
#[derive(Debug, Clone)]
enum SvgOpKind {
    Fill {
        fill_rule: FillRule,
    },
    Stroke {
        width: f32,
        line_cap: LineCap,
        line_join: LineJoin,
        miter_limit: f32,
        dash: Option<(Vec<f32>, f32)>,
    },
}

/// One draw operation of a *full-color* SVG — geometry in viewBox coordinates,
/// paired with the paint it was authored with, in **document order**.
///
/// This is the second of the icon's two geometry representations, and the
/// ordering is exactly why it has to exist. The tinted representation
/// ([`SvgIcon::raw_path`] + [`extra_fills`](SvgIcon::extra_fills) +
/// [`strokes`](SvgIcon::strokes)) merges every compatible shape into one path
/// and paints all fills before all strokes — sound when the whole icon is one
/// color, and wrong the moment two overlapping shapes have *different* colors,
/// since whichever is drawn last wins the overlap. Full-color rendering
/// therefore walks these ops in order instead.
#[derive(Debug, Clone)]
pub struct SvgOp {
    path: Path,
    kind: SvgOpKind,
    paint: SvgPaint,
    /// `opacity` × (`fill-opacity` | `stroke-opacity`), folded in at draw time.
    opacity: f32,
}

impl SvgOp {
    /// The paint this op was authored with.
    pub fn paint(&self) -> &SvgPaint {
        &self.paint
    }

    /// Whether this op strokes an outline (rather than filling an interior).
    pub fn is_stroke(&self) -> bool {
        matches!(self.kind, SvgOpKind::Stroke { .. })
    }
}

/// One ready-to-draw operation, fitted to a display rect — what a widget hands
/// straight to the canvas. Gradient coordinates are **path-bounds-local
/// pixels**, the space [`Canvas::fill_path`](crate::Canvas::fill_path) and
/// [`Canvas::stroke_path_with_paint`](crate::Canvas::stroke_path_with_paint)
/// expect.
#[derive(Debug, Clone)]
pub enum SvgDrawOp {
    Fill {
        path: Path,
        fill_rule: FillRule,
        paint: Paint,
    },
    Stroke {
        path: Path,
        style: StrokeStyle,
        paint: Paint,
    },
}

/// The resolved paint state for an element — what SVG's `fill` /
/// `stroke` presentation attributes would compute to. Inherited down the
/// element tree like real SVG. Not `Copy` (it carries a dash `Vec`); cloned
/// per child in the walk.
#[derive(Debug, Clone)]
struct SvgPaintState {
    /// How the shape's interior is painted ([`PaintRef::None`] = not at all).
    fill: PaintRef,
    /// How the shape's outline is painted ([`PaintRef::None`] = not at all).
    stroke: PaintRef,
    /// Stroke width in the element's local coordinate space.
    stroke_width: f32,
    line_cap: LineCap,
    line_join: LineJoin,
    /// `stroke-miterlimit`; 4.0 by default.
    miter_limit: f32,
    /// `fill-rule` — non-zero winding (default) vs even-odd.
    fill_rule: FillRule,
    /// `opacity` — inherited, accumulated product of ancestor group
    /// opacities (approximates group compositing as per-element alpha).
    opacity: f32,
    /// `fill-opacity` — this element's fill alpha multiplier.
    fill_opacity: f32,
    /// `stroke-opacity` — this element's stroke alpha multiplier.
    stroke_opacity: f32,
    /// `stroke-dasharray` (local-space lengths), if dashed.
    dash_array: Option<Vec<f32>>,
    /// `stroke-dashoffset` (local space).
    dash_offset: f32,
    /// `display:none` — prunes this element and its subtree. Not
    /// inherited (only ever set true by the element's own declaration;
    /// a pruned element's children are never visited).
    display_none: bool,
    /// `visibility` — inherited. `false` suppresses this element's own
    /// shape but children still process and may set `visibility:visible`.
    visible: bool,
}

impl Default for SvgPaintState {
    fn default() -> Self {
        // SVG initial values: fill = black (painted), stroke = none,
        // stroke-width = 1, butt caps, miter joins, non-zero fill rule,
        // fully opaque, no dash, displayed + visible.
        Self {
            fill: PaintRef::Color(Color::BLACK),
            stroke: PaintRef::None,
            stroke_width: 1.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            miter_limit: 4.0,
            fill_rule: FillRule::Winding,
            opacity: 1.0,
            fill_opacity: 1.0,
            stroke_opacity: 1.0,
            dash_array: None,
            dash_offset: 0.0,
            display_none: false,
            visible: true,
        }
    }
}

/// Shared, read-only context threaded through the element walk.
struct WalkCtx<'a> {
    /// Every `id`-bearing element in the document, for `<use>` resolution.
    id_map: &'a HashMap<&'a str, &'a XmlElement>,
    /// Parsed `<style>` CSS rules applied during paint-state resolution.
    css: &'a CssRules,
    /// Every gradient paint server in the document, by `id`.
    gradients: &'a HashMap<String, GradientDef>,
}

/// Bind a declared paint to the shape it paints.
///
/// A gradient reference is where the work is: `objectBoundingBox` units make it
/// a function of `local_bbox`, and `to_view_box` then carries the result into
/// the icon's coordinate space alongside the geometry.
///
/// Anything that can't be resolved — an id that isn't in the document, a
/// `<pattern>` (not implemented), a gradient with no stops — falls back to the
/// author's own fallback color if they wrote one, and otherwise to
/// [`SvgPaint::Current`]. Deliberately *not* to nothing: a shape that vanishes
/// is a much worse failure than a shape painted in the widget's tint, and the
/// tint is exactly what this pipeline did with every color before it could read
/// them.
fn resolve_paint(
    declared: &PaintRef,
    local_bbox: Rect,
    to_view_box: &Transform2D,
    ctx: &WalkCtx,
) -> SvgPaint {
    match declared {
        PaintRef::None => SvgPaint::Current, // unreachable: not painted
        PaintRef::Current => SvgPaint::Current,
        PaintRef::Color(c) => SvgPaint::Solid(*c),
        PaintRef::Ref { id, fallback } => ctx
            .gradients
            .get(id)
            .and_then(|g| g.resolve(local_bbox, to_view_box))
            .map(SvgPaint::Gradient)
            .unwrap_or_else(|| match fallback {
                Some(c) => SvgPaint::Solid(*c),
                None => SvgPaint::Current,
            }),
    }
}

/// Recursion bound — guards against `<use>` reference cycles and
/// pathologically deep documents (icons nest only a handful of levels).
const MAX_WALK_DEPTH: usize = 256;

/// Build the `id -> element` index over the whole tree (first definition
/// wins, matching SVG's duplicate-id resolution).
fn build_id_map<'a>(node: &'a XmlElement, map: &mut HashMap<&'a str, &'a XmlElement>) {
    if let Some(id) = node.attribute("id") {
        map.entry(id).or_insert(node);
    }
    for child in node.children() {
        build_id_map(child, map);
    }
}

/// Elements whose content is a definition / metadata, not direct geometry.
/// They (and their subtrees) must not paint when walked in document order.
/// `<symbol>`/`<defs>` content reaches the canvas only via `<use>`.
/// `<style>` text is consumed separately by the CSS pre-pass.
fn is_non_rendering(tag: &str) -> bool {
    matches!(
        tag,
        "defs"
            | "symbol"
            | "clipPath"
            | "mask"
            | "pattern"
            | "marker"
            | "style"
            | "metadata"
            | "title"
            | "desc"
            | "text"
            | "tspan"
            | "image"
            | "switch"
            // Paint servers: definitions, collected separately (see
            // `collect_gradients`). They reach the canvas through the shapes
            // that reference them, never by being walked.
            | "linearGradient"
            | "radialGradient"
            | "stop"
    )
}

fn walk_element(
    node: &XmlElement,
    parent_transform: &Transform2D,
    parent_paint: SvgPaintState,
    builder: &mut SvgBuilder,
    ctx: &WalkCtx,
    depth: usize,
) -> Result<(), SvgParseError> {
    if depth > MAX_WALK_DEPTH {
        return Ok(());
    }

    // Non-rendering containers: skip the whole subtree.
    if is_non_rendering(node.tag_name()) {
        #[cfg(debug_assertions)]
        if matches!(node.tag_name(), "text" | "tspan") {
            eprintln!(
                "teksilo: SVG <{}> is not supported by the icon parser; element ignored",
                node.tag_name()
            );
        }
        return Ok(());
    }

    // The element's own `transform` maps its local space into the parent's,
    // so it composes *inside* the parent chain: apply local first, then the
    // ancestors (`local.then(parent)`).
    let transform = if let Some(t_attr) = node.attribute("transform") {
        let local = parse_transform(t_attr)?;
        local.then(parent_transform)
    } else {
        *parent_transform
    };

    let paint = resolve_paint_state(node, parent_paint, ctx.css);

    // `display:none` prunes this element and its whole subtree.
    if paint.display_none {
        return Ok(());
    }

    // `<use>` instantiates a referenced element; it has no geometry of its
    // own and no rendered children.
    if node.tag_name() == "use" {
        instantiate_use(node, &transform, paint, builder, ctx, depth)?;
        return Ok(());
    }

    // Compute this element's shape in its local coordinate space (if it
    // is a drawable shape at all), then emit fill and/or stroke.
    let shape: Option<Path> = match node.tag_name() {
        "path" => match node.attribute("d") {
            Some(d) => {
                let mut p = Path::new();
                p.commands = path_parser::parse_svg_path_data(d)?;
                Some(p)
            }
            None => None,
        },
        "rect" => parse_rect_element(node),
        "circle" => parse_circle_element(node),
        "ellipse" => parse_ellipse_element(node),
        "line" => parse_line_element(node),
        "polygon" => parse_polygon_element(node),
        "polyline" => parse_polyline_element(node),
        _ => None,
    };

    // `visibility:hidden` suppresses this element's own shape (but not its
    // children — they recurse below and may set `visibility:visible`).
    if paint.visible
        && let Some(local) = shape
    {
        // A gradient in the default `objectBoundingBox` units is measured
        // against the shape's own box, in the shape's own (pre-transform)
        // space — so this is captured before the geometry is baked into the
        // viewBox. Per SVG, it is the *fill* box: a stroke does not widen it.
        let local_bbox = local.bounds();
        let world = if transform != Transform2D::IDENTITY {
            local.transformed(&transform)
        } else {
            local
        };
        // Local-space stroke widths / dash lengths bake into viewBox space
        // by the cumulative transform's scale so they track group scaling.
        let scale = transform.geometric_scale();
        if paint.fill.is_painted() {
            let fill_alpha = paint.opacity * paint.fill_opacity;
            builder.push_fill(world.clone(), paint.fill_rule, fill_alpha);
            builder.push_op(SvgOp {
                path: world.clone(),
                kind: SvgOpKind::Fill {
                    fill_rule: paint.fill_rule,
                },
                paint: resolve_paint(&paint.fill, local_bbox, &transform, ctx),
                opacity: fill_alpha,
            });
        }
        if paint.stroke.is_painted() && paint.stroke_width > 0.0 {
            let width = paint.stroke_width * scale;
            let stroke_alpha = paint.opacity * paint.stroke_opacity;
            let dash = paint.dash_array.as_ref().map(|arr| {
                (
                    arr.iter().map(|d| d * scale).collect::<Vec<f32>>(),
                    paint.dash_offset * scale,
                )
            });
            builder.push_stroke(
                world.clone(),
                width,
                paint.line_cap,
                paint.line_join,
                paint.miter_limit,
                stroke_alpha,
                dash.clone(),
            );
            builder.push_op(SvgOp {
                path: world,
                kind: SvgOpKind::Stroke {
                    width,
                    line_cap: paint.line_cap,
                    line_join: paint.line_join,
                    miter_limit: paint.miter_limit,
                    dash,
                },
                paint: resolve_paint(&paint.stroke, local_bbox, &transform, ctx),
                opacity: stroke_alpha,
            });
        }
    }

    // Recurse into children (for <g>, <svg>, <a>, etc.), passing the
    // resolved transform and paint state down so they inherit.
    for child in node.children() {
        walk_element(child, &transform, paint.clone(), builder, ctx, depth + 1)?;
    }

    Ok(())
}

/// Resolve and render a `<use>` reference. The `use_transform` already
/// folds in ancestors + the `<use>` element's own `transform`; the
/// element's `x`/`y` add an inner translation, and a referenced
/// `<symbol>`/`<svg>` viewport maps its `viewBox` into the `<use>` size box.
fn instantiate_use(
    node: &XmlElement,
    use_transform: &Transform2D,
    paint: SvgPaintState,
    builder: &mut SvgBuilder,
    ctx: &WalkCtx,
    depth: usize,
) -> Result<(), SvgParseError> {
    let Some(href) = node
        .attribute("href")
        .or_else(|| node.attribute("xlink:href"))
    else {
        return Ok(());
    };
    let id = href.strip_prefix('#').unwrap_or(href);
    let Some(&target) = ctx.id_map.get(id) else {
        return Ok(()); // dangling reference — ignore, don't error
    };

    // `<use>` x/y is an inner translation, applied before the placement.
    let x = attr_f32(node, "x").unwrap_or(0.0);
    let y = attr_f32(node, "y").unwrap_or(0.0);
    let placed = Transform2D::translate(x, y).then(use_transform);

    match target.tag_name() {
        // A symbol/svg target renders its *children* through the symbol
        // viewport; the element itself never paints directly.
        "symbol" | "svg" => {
            let inner = symbol_viewport_transform(target, node, &placed);
            let tpaint = resolve_paint_state(target, paint, ctx.css);
            for child in target.children() {
                walk_element(child, &inner, tpaint.clone(), builder, ctx, depth + 1)?;
            }
        }
        // Any other element (shape, <g>, …) renders directly at the placement.
        _ => {
            walk_element(target, &placed, paint, builder, ctx, depth + 1)?;
        }
    }
    Ok(())
}

/// Transform mapping a referenced `<symbol>`/`<svg>`'s `viewBox` content
/// into the `<use>` size box. Only scales when the symbol declares a
/// `viewBox` *and* the `<use>` declares `width`+`height`; otherwise the
/// symbol is treated as a plain group (the common icon-sprite case where
/// the symbol's coordinate system already matches the outer viewBox).
fn symbol_viewport_transform(
    symbol: &XmlElement,
    use_node: &XmlElement,
    placed: &Transform2D,
) -> Transform2D {
    let vb = symbol.attribute("viewBox").and_then(parse_view_box_values);
    match (
        vb,
        attr_f32(use_node, "width"),
        attr_f32(use_node, "height"),
    ) {
        (Some((vx, vy, vw, vh)), Some(uw), Some(uh)) if vw > 0.0 && vh > 0.0 => {
            let vb_local =
                Transform2D::translate(-vx, -vy).then(&Transform2D::scale(uw / vw, uh / vh));
            vb_local.then(placed)
        }
        _ => *placed,
    }
}

/// Parse a `preserveAspectRatio` value (e.g. `xMidYMid meet`, `none`,
/// `xMinYMax slice`). Unknown tokens fall back to the `xMidYMid meet`
/// default.
fn parse_preserve_aspect_ratio(s: &str) -> AspectRatio {
    let mut it = s.split_whitespace();
    let align = it.next().unwrap_or("xMidYMid");
    if align.eq_ignore_ascii_case("none") {
        return AspectRatio {
            stretch: true,
            ..AspectRatio::default()
        };
    }
    let slice = it.next().is_some_and(|m| m.eq_ignore_ascii_case("slice"));
    let x = if align.contains("xMin") {
        Align::Min
    } else if align.contains("xMax") {
        Align::Max
    } else {
        Align::Mid
    };
    let y = if align.contains("YMin") {
        Align::Min
    } else if align.contains("YMax") {
        Align::Max
    } else {
        Align::Mid
    };
    AspectRatio {
        x,
        y,
        slice,
        stretch: false,
    }
}

/// Parse a `viewBox` attribute value into `(x, y, w, h)`; `None` if it
/// isn't exactly four numbers.
fn parse_view_box_values(vb: &str) -> Option<(f32, f32, f32, f32)> {
    let nums: Vec<f32> = vb
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<f32>().ok())
        .collect();
    if nums.len() == 4 {
        Some((nums[0], nums[1], nums[2], nums[3]))
    } else {
        None
    }
}

/// The presentation attributes the icon parser reads from an element.
const PAINT_PROPERTIES: &[&str] = &[
    "fill",
    "stroke",
    "stroke-width",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "fill-rule",
    "opacity",
    "fill-opacity",
    "stroke-opacity",
    "stroke-dasharray",
    "stroke-dashoffset",
    "display",
    "visibility",
];

/// Resolve an element's paint state from the inherited parent state plus,
/// in ascending precedence: presentation attributes < `<style>` CSS rules
/// < the inline `style` attribute — matching SVG's cascade for these
/// properties.
fn resolve_paint_state(node: &XmlElement, parent: SvgPaintState, css: &CssRules) -> SvgPaintState {
    let mut st = parent;
    // `display`/`visibility` are per-element decisions, not inherited as
    // a pruning/suppression flag — reset before applying this element's
    // own declarations. (`visible` *content* still inherited via `parent`.)
    st.display_none = false;

    // 1. Presentation attributes (CSS specificity 0).
    for &prop in PAINT_PROPERTIES {
        if let Some(v) = node.attribute(prop) {
            apply_declaration(&mut st, prop, v);
        }
    }

    // 2. `<style>` rule declarations, by ascending selector specificity.
    css.apply_to(&mut st, node);

    // 3. Inline `style` attribute (highest precedence).
    if let Some(style) = node.attribute("style") {
        for (key, value) in parse_inline_style(style) {
            apply_declaration(&mut st, key, value);
        }
    }

    st
}

/// Apply one `property: value` declaration onto the paint state. Shared by
/// presentation attributes, `<style>` rules, and the inline `style` attr.
fn apply_declaration(st: &mut SvgPaintState, key: &str, value: &str) {
    match key {
        // An unreadable paint (or an explicit `inherit`) leaves the inherited
        // value in place, as CSS requires — it must not fall back to black.
        "fill" => {
            if let Some(p) = parse_paint_ref(value) {
                st.fill = p;
            }
        }
        "stroke" => {
            if let Some(p) = parse_paint_ref(value) {
                st.stroke = p;
            }
        }
        "stroke-width" => {
            if let Some(w) = parse_length(value) {
                st.stroke_width = w;
            }
        }
        "stroke-linecap" => st.line_cap = parse_line_cap(value),
        "stroke-linejoin" => st.line_join = parse_line_join(value),
        "stroke-miterlimit" => {
            if let Some(m) = value.trim().parse::<f32>().ok().filter(|m| *m >= 1.0) {
                st.miter_limit = m;
            }
        }
        // `vector-effect="non-scaling-stroke"` is intentionally NOT mapped:
        // for a fixed-size icon render it would freeze the stroke width
        // against the icon's own fit scale (hairline at large sizes), which
        // is rarely what an icon author wants. Documented as a limitation.
        "fill-rule" => {
            st.fill_rule = if value.trim().eq_ignore_ascii_case("evenodd") {
                FillRule::EvenOdd
            } else {
                FillRule::Winding
            };
        }
        // `opacity` is a group multiplier: it compounds with the inherited
        // value (and with fill/stroke-opacity) rather than replacing it.
        "opacity" => {
            if let Some(o) = parse_opacity(value) {
                st.opacity *= o;
            }
        }
        "fill-opacity" => {
            if let Some(o) = parse_opacity(value) {
                st.fill_opacity = o;
            }
        }
        "stroke-opacity" => {
            if let Some(o) = parse_opacity(value) {
                st.stroke_opacity = o;
            }
        }
        "stroke-dasharray" => st.dash_array = parse_dash_array(value),
        "stroke-dashoffset" => {
            if let Some(off) = parse_length(value) {
                st.dash_offset = off;
            }
        }
        "display" => st.display_none = value.trim().eq_ignore_ascii_case("none"),
        "visibility" => {
            let v = value.trim();
            if v.eq_ignore_ascii_case("hidden") || v.eq_ignore_ascii_case("collapse") {
                st.visible = false;
            } else if v.eq_ignore_ascii_case("visible") {
                st.visible = true;
            }
        }
        _ => {}
    }
}

/// Parse a `fill` / `stroke` value into a [`PaintRef`].
///
/// `None` means "this declaration says nothing" — an explicit `inherit`, or a
/// value the parser can't read. Both must leave the inherited paint alone
/// (returning a black `PaintRef::Color` for an unreadable value would repaint
/// the element, which is worse than ignoring it).
fn parse_paint_ref(value: &str) -> Option<PaintRef> {
    let v = value.trim();
    if v.eq_ignore_ascii_case("none") {
        return Some(PaintRef::None);
    }
    if v.eq_ignore_ascii_case("currentcolor") {
        return Some(PaintRef::Current);
    }
    if v.eq_ignore_ascii_case("inherit") {
        return None;
    }
    if let Some(rest) = v.strip_prefix("url(").or_else(|| v.strip_prefix("URL(")) {
        let (inside, after) = rest.split_once(')')?;
        let id = inside.trim().trim_matches(['"', '\'']).trim();
        let id = id.strip_prefix('#').unwrap_or(id);
        if id.is_empty() {
            return None;
        }
        // CSS allows a fallback paint after the reference: `url(#g) red`.
        let fallback = match after.trim() {
            "" => None,
            f if f.eq_ignore_ascii_case("none") => return Some(PaintRef::None),
            f => parse_color(f),
        };
        return Some(PaintRef::Ref {
            id: id.to_string(),
            fallback,
        });
    }
    parse_color(v).map(PaintRef::Color)
}

fn parse_line_cap(value: &str) -> LineCap {
    match value.trim() {
        "round" => LineCap::Round,
        "square" => LineCap::Square,
        _ => LineCap::Butt,
    }
}

fn parse_line_join(value: &str) -> LineJoin {
    match value.trim() {
        "round" => LineJoin::Round,
        "bevel" => LineJoin::Bevel,
        _ => LineJoin::Miter,
    }
}

/// Parse an opacity value — a number or a percentage — clamped to `[0, 1]`.
fn parse_opacity(value: &str) -> Option<f32> {
    let v = value.trim();
    let n = if let Some(pct) = v.strip_suffix('%') {
        pct.trim().parse::<f32>().ok()? / 100.0
    } else {
        v.parse::<f32>().ok()?
    };
    Some(n.clamp(0.0, 1.0))
}

/// Parse a `stroke-dasharray` value into local-space lengths. `none` /
/// empty / all-zero → `None` (solid). An odd-length list is doubled per
/// the SVG spec.
fn parse_dash_array(value: &str) -> Option<Vec<f32>> {
    let v = value.trim();
    if v.is_empty() || v.eq_ignore_ascii_case("none") {
        return None;
    }
    let mut nums: Vec<f32> = v
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .filter_map(parse_length)
        .filter(|n| *n >= 0.0)
        .collect();
    if nums.is_empty() || nums.iter().all(|n| *n == 0.0) {
        return None;
    }
    if nums.len() % 2 == 1 {
        let dup = nums.clone();
        nums.extend(dup);
    }
    Some(nums)
}

/// Split an inline `style="a:b;c:d"` attribute (or a CSS declaration
/// block body) into `(property, value)` pairs, trimmed. Declarations
/// without a `:` are skipped.
fn parse_inline_style(style: &str) -> impl Iterator<Item = (&str, &str)> {
    style.split(';').filter_map(|decl| {
        let (key, value) = decl.split_once(':')?;
        Some((key.trim(), value.trim()))
    })
}

// --- Minimal CSS (`<style>` blocks + `class`) -------------------------
//
// Illustrator / Inkscape / Font-Awesome exports style geometry via CSS
// classes rather than presentation attributes. Without this, those
// elements fall back to defaults (fill on, stroke off) and line icons
// render as filled blobs. Scope is deliberately narrow: single tag /
// `.class` / `#id` / `*` selectors only — anything with a combinator,
// pseudo-class, attribute selector, or compound form is skipped.

/// A supported simple CSS selector.
enum CssSelector {
    Universal,
    Tag(String),
    Class(String),
    Id(String),
}

/// One CSS rule: a selector plus its ordered declarations.
struct CssRule {
    selector: CssSelector,
    decls: Vec<(String, String)>,
}

impl CssRule {
    fn matches(&self, tag: &str, classes: &[&str], id: Option<&str>) -> bool {
        match &self.selector {
            CssSelector::Universal => true,
            CssSelector::Tag(t) => t == tag,
            CssSelector::Class(c) => classes.contains(&c.as_str()),
            CssSelector::Id(i) => id == Some(i.as_str()),
        }
    }
}

/// The parsed `<style>` rule set for a document.
#[derive(Default)]
struct CssRules {
    rules: Vec<CssRule>,
}

impl CssRules {
    /// Apply every matching rule's declarations to `st`, in ascending
    /// specificity (universal/tag, then class, then id) and document
    /// order within a tier — so id rules win over class rules win over
    /// tag rules, the practical subset of CSS specificity.
    fn apply_to(&self, st: &mut SvgPaintState, node: &XmlElement) {
        let tag = node.tag_name();
        let id = node.attribute("id");
        let class_attr = node.attribute("class").unwrap_or("");
        let classes: Vec<&str> = class_attr.split_whitespace().collect();

        let tiers = [
            |s: &CssSelector| matches!(s, CssSelector::Universal | CssSelector::Tag(_)),
            |s: &CssSelector| matches!(s, CssSelector::Class(_)),
            |s: &CssSelector| matches!(s, CssSelector::Id(_)),
        ];
        for in_tier in tiers {
            for rule in &self.rules {
                if in_tier(&rule.selector) && rule.matches(tag, &classes, id) {
                    for (k, v) in &rule.decls {
                        apply_declaration(st, k, v);
                    }
                }
            }
        }
    }
}

/// Collect and parse every `<style>` element's CSS in the document
/// (top-level or nested, e.g. inside `<defs>`).
fn collect_style_rules(svg_el: &XmlElement) -> CssRules {
    let mut rules = Vec::new();
    collect_style_text(svg_el, &mut rules);
    CssRules { rules }
}

fn collect_style_text(node: &XmlElement, rules: &mut Vec<CssRule>) {
    if node.tag_name() == "style" {
        // Treat as CSS only when the type is CSS or unspecified.
        let is_css = node
            .attribute("type")
            .map(|t| t.trim().eq_ignore_ascii_case("text/css"))
            .unwrap_or(true);
        if is_css {
            let stripped = strip_css_comments(node.text_content());
            parse_css_block(&stripped, rules);
        }
    }
    for child in node.children() {
        collect_style_text(child, rules);
    }
}

/// Strip `/* … */` comments from a CSS string.
fn strip_css_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => {
                rest = ""; // unterminated comment — drop the remainder
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Parse `selector { decls } …` rules, appending to `rules`. `@`-rules
/// (`@charset`/`@import`/`@media`/`@keyframes`/…) are skipped — bodyless
/// ones to their `;`, block ones past their matching `}` — so a stray
/// at-rule (common in Illustrator / Inkscape exports) doesn't poison the
/// following selector or silently drop the rest of the sheet.
fn parse_css_block(text: &str, rules: &mut Vec<CssRule>) {
    let mut rest = text;
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        if rest.starts_with('@') {
            rest = skip_at_rule(rest);
            continue;
        }
        let Some(open) = rest.find('{') else {
            break;
        };
        let selector_part = rest[..open].trim();
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            break; // unbalanced — stop
        };
        let decl_block = &after[..close];
        rest = &after[close + 1..];

        if selector_part.is_empty() {
            continue;
        }
        let decls: Vec<(String, String)> = parse_inline_style(decl_block)
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        if decls.is_empty() {
            continue;
        }
        for sel in selector_part.split(',') {
            if let Some(selector) = parse_simple_selector(sel.trim()) {
                rules.push(CssRule {
                    selector,
                    decls: decls.clone(),
                });
            }
        }
    }
}

/// Skip a CSS at-rule starting at `s[0] == '@'`. Bodyless rules
/// (`@charset`/`@import`) end at `;`; block rules (`@media`/`@keyframes`)
/// end past their balanced `{ }`. Returns the remainder after the rule.
fn skip_at_rule(s: &str) -> &str {
    let brace = s.find('{');
    let semi = s.find(';');
    match (brace, semi) {
        // Bodyless at-rule: no block, or `;` before any block.
        (None, Some(sc)) => &s[sc + 1..],
        (Some(b), Some(sc)) if sc < b => &s[sc + 1..],
        // Block at-rule: skip past its matching close brace.
        (Some(b), _) => skip_balanced_braces(&s[b..]),
        (None, None) => "", // malformed — consume the remainder
    }
}

/// Given a slice starting at an opening `{`, return the slice after the
/// matching `}` (or `""` if unbalanced).
fn skip_balanced_braces(s: &str) -> &str {
    let mut depth: u32 = 0;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &s[i + 1..];
                }
            }
            _ => {}
        }
    }
    "" // unbalanced — consume the remainder
}

/// Parse a single simple selector (`*`, `tag`, `.class`, `#id`). Returns
/// `None` for compound / combinator / pseudo / attribute selectors.
fn parse_simple_selector(sel: &str) -> Option<CssSelector> {
    let sel = sel.trim();
    if sel.is_empty() {
        return None;
    }
    if sel == "*" {
        return Some(CssSelector::Universal);
    }
    // Reject anything carrying a combinator, pseudo, or attribute selector.
    if sel.contains([' ', '\t', '\n', '>', '+', '~', '[', ']', ':', '(']) {
        return None;
    }
    if let Some(class) = sel.strip_prefix('.') {
        // A bare `.class` — reject compound forms like `.a.b`.
        if class.is_empty() || class.contains(['.', '#']) {
            return None;
        }
        return Some(CssSelector::Class(class.to_string()));
    }
    if let Some(id) = sel.strip_prefix('#') {
        if id.is_empty() || id.contains(['.', '#']) {
            return None;
        }
        return Some(CssSelector::Id(id.to_string()));
    }
    // Bare tag name — reject compound forms like `rect.cls`.
    if sel.contains(['.', '#']) {
        return None;
    }
    Some(CssSelector::Tag(sel.to_string()))
}

// --- Shape element parsers ---

fn parse_rect_element(node: &XmlElement) -> Option<Path> {
    let x = attr_f32(node, "x").unwrap_or(0.0);
    let y = attr_f32(node, "y").unwrap_or(0.0);
    let w = attr_f32(node, "width")?;
    let h = attr_f32(node, "height")?;
    // Per SVG: a missing rx/ry takes the other's value.
    let (mut rx, mut ry) = match (attr_f32(node, "rx"), attr_f32(node, "ry")) {
        (Some(rx), Some(ry)) => (rx, ry),
        (Some(rx), None) => (rx, rx),
        (None, Some(ry)) => (ry, ry),
        (None, None) => (0.0, 0.0),
    };
    // Clamp each radius to half the corresponding side.
    rx = rx.clamp(0.0, w / 2.0);
    ry = ry.clamp(0.0, h / 2.0);
    if rx > 0.0 && ry > 0.0 {
        Some(rounded_rect_xy(Rect::new(x, y, w, h), rx, ry))
    } else {
        Some(Path::rect(Rect::new(x, y, w, h)))
    }
}

/// Build a rounded-rectangle path with independent corner radii `rx`/`ry`
/// (true elliptical corners) — SVG `<rect rx=.. ry=..>`. Mirrors
/// `Path::rounded_rect`'s arc layout but with `2rx × 2ry` corner ellipses.
fn rounded_rect_xy(r: Rect, rx: f32, ry: f32) -> Path {
    let (x, y, right, bottom) = (r.x, r.y, r.right(), r.bottom());
    let (dx, dy) = (2.0 * rx, 2.0 * ry);
    let mut p = Path::new();
    p.move_to(Point::new(x + rx, y));
    p.line_to(Point::new(right - rx, y));
    p.arc_to(Rect::new(right - dx, y, dx, dy), -90.0, 90.0); // top-right
    p.line_to(Point::new(right, bottom - ry));
    p.arc_to(Rect::new(right - dx, bottom - dy, dx, dy), 0.0, 90.0); // bottom-right
    p.line_to(Point::new(x + rx, bottom));
    p.arc_to(Rect::new(x, bottom - dy, dx, dy), 90.0, 90.0); // bottom-left
    p.line_to(Point::new(x, y + ry));
    p.arc_to(Rect::new(x, y, dx, dy), 180.0, 90.0); // top-left
    p.close();
    p
}

fn parse_circle_element(node: &XmlElement) -> Option<Path> {
    let cx = attr_f32(node, "cx").unwrap_or(0.0);
    let cy = attr_f32(node, "cy").unwrap_or(0.0);
    let r = attr_f32(node, "r")?;
    Some(Path::circle(Point::new(cx, cy), r))
}

fn parse_ellipse_element(node: &XmlElement) -> Option<Path> {
    let cx = attr_f32(node, "cx").unwrap_or(0.0);
    let cy = attr_f32(node, "cy").unwrap_or(0.0);
    let rx = attr_f32(node, "rx")?;
    let ry = attr_f32(node, "ry")?;
    Some(Path::ellipse(Rect::new(
        cx - rx,
        cy - ry,
        rx * 2.0,
        ry * 2.0,
    )))
}

fn parse_line_element(node: &XmlElement) -> Option<Path> {
    let x1 = attr_f32(node, "x1").unwrap_or(0.0);
    let y1 = attr_f32(node, "y1").unwrap_or(0.0);
    let x2 = attr_f32(node, "x2").unwrap_or(0.0);
    let y2 = attr_f32(node, "y2").unwrap_or(0.0);
    Some(Path::line(Point::new(x1, y1), Point::new(x2, y2)))
}

fn parse_polygon_element(node: &XmlElement) -> Option<Path> {
    let points = parse_points_attr(node)?;
    Some(Path::polygon(&points))
}

fn parse_polyline_element(node: &XmlElement) -> Option<Path> {
    let points = parse_points_attr(node)?;
    if points.is_empty() {
        return None;
    }
    let mut path = Path::new();
    path.move_to(points[0]);
    for &p in &points[1..] {
        path.line_to(p);
    }
    Some(path)
}

fn parse_points_attr(node: &XmlElement) -> Option<Vec<Point>> {
    let raw = node.attribute("points")?;
    let nums: Vec<f32> = raw
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<f32>().ok())
        .collect();
    if nums.len() < 4 || !nums.len().is_multiple_of(2) {
        return None;
    }
    Some(nums.chunks(2).map(|c| Point::new(c[0], c[1])).collect())
}

fn attr_f32(node: &XmlElement, name: &str) -> Option<f32> {
    node.attribute(name)?.parse::<f32>().ok()
}

// --- Transform parsing ---

fn parse_transform(attr: &str) -> Result<Transform2D, SvgParseError> {
    let mut result = Transform2D::IDENTITY;
    let mut remaining = attr.trim();

    while !remaining.is_empty() {
        remaining = remaining.trim_start();
        if remaining.is_empty() {
            break;
        }

        // Reduce each token to a single matrix `op`. SVG applies the
        // leftmost token *outermost* (`transform="A B"` ⇒ matrix `A·B`,
        // a point transformed as `A·B·p` ⇒ B first), so each new token
        // appends on the RIGHT of the running product: `result = op · result`,
        // which with `a.then(&b) == b·a` is `op.then(&result)`.
        let (op, rest) = if let Some(rest) = remaining.strip_prefix("translate") {
            let (args, rest) = parse_transform_args(rest)?;
            let tx = args.first().copied().unwrap_or(0.0);
            let ty = args.get(1).copied().unwrap_or(0.0);
            (Transform2D::translate(tx, ty), rest)
        } else if let Some(rest) = remaining.strip_prefix("scale") {
            let (args, rest) = parse_transform_args(rest)?;
            let sx = args.first().copied().unwrap_or(1.0);
            let sy = args.get(1).copied().unwrap_or(sx);
            (Transform2D::scale(sx, sy), rest)
        } else if let Some(rest) = remaining.strip_prefix("rotate") {
            let (args, rest) = parse_transform_args(rest)?;
            let angle = args.first().copied().unwrap_or(0.0).to_radians();
            let op = if args.len() >= 3 {
                // rotate(a cx cy) = translate(cx,cy)·rotate(a)·translate(-cx,-cy):
                // move the pivot to the origin, rotate, move it back.
                let (cx, cy) = (args[1], args[2]);
                Transform2D::translate(-cx, -cy)
                    .then(&Transform2D::rotate(angle))
                    .then(&Transform2D::translate(cx, cy))
            } else {
                Transform2D::rotate(angle)
            };
            (op, rest)
        } else if let Some(rest) = remaining.strip_prefix("matrix") {
            let (args, rest) = parse_transform_args(rest)?;
            if args.len() != 6 {
                return Err(SvgParseError::InvalidTransform(
                    "matrix requires 6 values".into(),
                ));
            }
            (
                Transform2D {
                    m: [args[0], args[1], args[2], args[3], args[4], args[5]],
                },
                rest,
            )
        } else if let Some(rest) = remaining.strip_prefix("skewX") {
            let (args, rest) = parse_transform_args(rest)?;
            let angle = args.first().copied().unwrap_or(0.0).to_radians();
            (
                Transform2D {
                    m: [1.0, 0.0, angle.tan(), 1.0, 0.0, 0.0],
                },
                rest,
            )
        } else if let Some(rest) = remaining.strip_prefix("skewY") {
            let (args, rest) = parse_transform_args(rest)?;
            let angle = args.first().copied().unwrap_or(0.0).to_radians();
            (
                Transform2D {
                    m: [1.0, angle.tan(), 0.0, 1.0, 0.0, 0.0],
                },
                rest,
            )
        } else {
            return Err(SvgParseError::InvalidTransform(format!(
                "unrecognized transform: {remaining}"
            )));
        };

        result = op.then(&result);

        // Skip optional comma/whitespace between transforms.
        remaining = rest.trim_start();
        remaining = remaining.strip_prefix(',').unwrap_or(remaining);
    }

    Ok(result)
}

fn parse_transform_args(s: &str) -> Result<(Vec<f32>, &str), SvgParseError> {
    let s = s.trim_start();
    let s = s.strip_prefix('(').ok_or_else(|| {
        SvgParseError::InvalidTransform("expected '(' after transform function".into())
    })?;
    let end = s
        .find(')')
        .ok_or_else(|| SvgParseError::InvalidTransform("missing ')' in transform".into()))?;
    let inner = &s[..end];
    let rest = &s[end + 1..];
    let args: Vec<f32> = inner
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse::<f32>()
                .map_err(|_| SvgParseError::InvalidTransform(format!("invalid number '{s}'")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((args, rest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::PathCommand;

    #[test]
    fn parse_simple_svg() {
        let svg = r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
            <path d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!((icon.width() - 24.0).abs() < 0.01);
        assert!((icon.height() - 24.0).abs() < 0.01);
        assert!(!icon.raw_path().is_empty());
    }

    #[test]
    fn parse_svg_with_rect() {
        let svg = r#"<svg viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg">
            <rect x="10" y="10" width="80" height="80"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(!icon.raw_path().is_empty());
        // rect → 5 commands (MoveTo + 3 LineTo + Close)
        assert_eq!(icon.raw_path().commands.len(), 5);
    }

    #[test]
    fn parse_svg_with_circle() {
        let svg = r#"<svg viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg">
            <circle cx="50" cy="50" r="40"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(!icon.raw_path().is_empty());
    }

    #[test]
    fn parse_svg_with_group_transform() {
        let svg = r#"<svg viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg">
            <g transform="translate(10,20)">
                <line x1="0" y1="0" x2="50" y2="50"/>
            </g>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        let cmds = &icon.raw_path().commands;
        assert_eq!(cmds.len(), 2);
        match cmds[0] {
            PathCommand::MoveTo(p) => {
                assert!((p.x - 10.0).abs() < 0.01);
                assert!((p.y - 20.0).abs() < 0.01);
            }
            _ => panic!("expected MoveTo"),
        }
        match cmds[1] {
            PathCommand::LineTo(p) => {
                assert!((p.x - 60.0).abs() < 0.01);
                assert!((p.y - 70.0).abs() < 0.01);
            }
            _ => panic!("expected LineTo"),
        }
    }

    #[test]
    fn to_path_scales_to_size() {
        let svg = r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
            <rect x="0" y="0" width="24" height="24"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        let path = icon.to_path(48.0);
        let bounds = path.bounds();
        // Should be scaled to roughly 48x48
        assert!((bounds.width - 48.0).abs() < 1.0);
        assert!((bounds.height - 48.0).abs() < 1.0);
    }

    #[test]
    fn parse_svg_width_height_fallback() {
        let svg = r#"<svg width="16" height="16" xmlns="http://www.w3.org/2000/svg">
            <path d="M0 0L16 16"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!((icon.width() - 16.0).abs() < 0.01);
    }

    #[test]
    fn parse_svg_polygon() {
        let svg = r#"<svg viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg">
            <polygon points="50,0 100,100 0,100"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        // polygon with 3 points → MoveTo + 2 LineTo + Close = 4
        assert_eq!(icon.raw_path().commands.len(), 4);
    }

    #[test]
    fn stroked_circle_subpath_opens_with_moveto() {
        // Regression: a stroked `<circle>` must produce a subpath that opens with
        // a `MoveTo`. Without it a renderer has no start point for the arc and
        // draws a stray line from the origin to the circle — a diagonal line
        // through a sun/gear icon from its top-left corner.
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor">
            <circle cx="8" cy="8" r="2.3"/>
        </svg>"##;
        let icon = SvgIcon::parse(svg).unwrap();
        assert_eq!(
            icon.strokes.len(),
            1,
            "the stroked circle should yield one stroke"
        );
        assert!(
            matches!(
                icon.strokes[0].path.commands.first(),
                Some(PathCommand::MoveTo(_))
            ),
            "a stroked circle subpath must open with MoveTo, got {:?}",
            icon.strokes[0].path.commands.first()
        );
    }

    #[test]
    fn parse_svg_ellipse() {
        let svg = r#"<svg viewBox="0 0 200 100" xmlns="http://www.w3.org/2000/svg">
            <ellipse cx="100" cy="50" rx="80" ry="40"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(!icon.raw_path().is_empty());
    }

    #[test]
    fn parse_svg_multiple_paths() {
        let svg = r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
            <path d="M0 0L10 10"/>
            <path d="M20 20L24 24"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        // Two paths merged: 2 + 2 = 4 commands
        assert_eq!(icon.raw_path().commands.len(), 4);
    }

    #[test]
    fn to_path_with_nonzero_viewbox_origin() {
        // viewBox starts at (10, 10), icon content at (10, 10)-(34, 34)
        let svg = r#"<svg viewBox="10 10 24 24" xmlns="http://www.w3.org/2000/svg">
            <rect x="10" y="10" width="24" height="24"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        let path = icon.to_path(48.0);
        let bounds = path.bounds();
        // The rect should be scaled to 48x48 and placed at (0,0)
        assert!(bounds.x.abs() < 1.0, "x should be near 0, got {}", bounds.x);
        assert!(bounds.y.abs() < 1.0, "y should be near 0, got {}", bounds.y);
        assert!(
            (bounds.width - 48.0).abs() < 1.0,
            "width should be ~48, got {}",
            bounds.width
        );
        assert!(
            (bounds.height - 48.0).abs() < 1.0,
            "height should be ~48, got {}",
            bounds.height
        );
    }

    #[test]
    fn missing_svg_element() {
        let result = SvgIcon::parse("<html></html>");
        assert!(result.is_err());
    }

    #[test]
    fn missing_viewbox() {
        let result =
            SvgIcon::parse(r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0"/></svg>"#);
        assert!(result.is_err());
    }

    #[test]
    fn material_design_home_icon() {
        // Material Design "home" icon
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
            <path d="M10 20v-6h4v6h5v-8h3L12 3 2 12h3v8z"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(!icon.raw_path().is_empty());
        let path = icon.to_path(24.0);
        assert!(!path.is_empty());
    }

    // ── Stroke (line-style icon) support ──────────────────────────────

    #[test]
    fn filled_icon_has_no_strokes() {
        // A plain icon with no fill/stroke attributes keeps the historic
        // behavior: everything is filled, nothing is stroked.
        let svg = r#"<svg viewBox="0 0 24 24"><path d="M0 0L24 0L24 24Z"/></svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(!icon.raw_path().is_empty());
        assert!(
            icon.strokes().is_empty(),
            "plain fill icon should not emit strokes"
        );
    }

    #[test]
    fn line_style_icon_strokes_not_fills() {
        // The Feather / Lucide / Tabler convention: stroke on the root,
        // fill="none", bare shape children inheriting. Without stroke
        // support these outlines would be filled into solid blobs.
        let svg = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="12" r="10"/>
            <line x1="12" y1="8" x2="12" y2="16"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(
            icon.raw_path().is_empty(),
            "line-style icon must have no fill geometry (else outlines become blobs)"
        );
        // circle + line share width / cap / join → merged into one group.
        assert_eq!(icon.strokes().len(), 1);
        let stroke = &icon.strokes()[0];
        assert!((stroke.width - 2.0).abs() < 0.01);
        assert_eq!(stroke.line_cap, LineCap::Round);
        assert_eq!(stroke.line_join, LineJoin::Round);
        assert!(!icon.is_empty());
    }

    #[test]
    fn element_with_fill_and_stroke_emits_both() {
        let svg = r#"<svg viewBox="0 0 24 24">
            <rect x="2" y="2" width="20" height="20" fill="white" stroke="black" stroke-width="1"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(!icon.raw_path().is_empty(), "filled rect should fill");
        assert_eq!(icon.strokes().len(), 1, "bordered rect should also stroke");
    }

    #[test]
    fn inline_style_drives_stroke() {
        let svg = r#"<svg viewBox="0 0 24 24">
            <path d="M2 12L22 12" style="fill:none;stroke:#333;stroke-width:3"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(
            icon.raw_path().is_empty(),
            "fill:none in inline style must suppress fill"
        );
        assert_eq!(icon.strokes().len(), 1);
        assert!((icon.strokes()[0].width - 3.0).abs() < 0.01);
    }

    #[test]
    fn stroke_inherits_through_group_with_override() {
        let svg = r#"<svg viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2">
            <g stroke-width="4">
                <line x1="0" y1="0" x2="10" y2="0"/>
            </g>
            <line x1="0" y1="10" x2="10" y2="10"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        // Distinct widths (4 inside the group, 2 outside) → two groups.
        assert_eq!(icon.strokes().len(), 2);
        let mut widths: Vec<f32> = icon.strokes().iter().map(|s| s.width).collect();
        widths.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!((widths[0] - 2.0).abs() < 0.01);
        assert!((widths[1] - 4.0).abs() < 0.01);
    }

    #[test]
    fn group_scale_transform_scales_stroke_width() {
        let svg = r#"<svg viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2">
            <g transform="scale(2)">
                <line x1="0" y1="0" x2="5" y2="0"/>
            </g>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert_eq!(icon.strokes().len(), 1);
        // width 2 in local space × 2 group scale → 4 in viewBox space.
        assert!((icon.strokes()[0].width - 4.0).abs() < 0.01);
    }

    #[test]
    fn stroked_paths_scale_width_to_display() {
        let svg = r#"<svg viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2">
            <line x1="0" y1="0" x2="24" y2="0"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        // Fit into 48×48 → 2× scale → display stroke width 4.
        let strokes = icon.stroked_paths_in_rect(Rect::new(0.0, 0.0, 48.0, 48.0));
        assert_eq!(strokes.len(), 1);
        assert!(
            (strokes[0].1.width - 4.0).abs() < 0.01,
            "stroke width must scale with the icon, got {}",
            strokes[0].1.width
        );
    }

    #[test]
    fn stroke_none_suppresses_stroke() {
        // An explicit stroke="none" on a child overrides an inherited
        // stroke from the root.
        let svg = r#"<svg viewBox="0 0 24 24" stroke="black" stroke-width="2" fill="none">
            <rect x="2" y="2" width="20" height="20" stroke="none" fill="black"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(!icon.raw_path().is_empty(), "fill='black' should fill");
        assert!(
            icon.strokes().is_empty(),
            "stroke='none' child must not stroke"
        );
    }

    // ── Non-rendering containers, <use>/<symbol>, transform order, guards ──

    #[test]
    fn nested_transforms_compose_in_svg_order() {
        // A child transform applies BEFORE the parent's (local.then(parent)):
        // inner scale(2) then outer translate(10,0): (5,5) → (10,10) → (20,10).
        let svg = r#"<svg viewBox="0 0 100 100">
            <g transform="translate(10,0)">
                <g transform="scale(2)">
                    <rect x="5" y="5" width="1" height="1"/>
                </g>
            </g>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        match icon.raw_path().commands.first() {
            Some(PathCommand::MoveTo(p)) => {
                assert!((p.x - 20.0).abs() < 0.01, "x should be 20, got {}", p.x);
                assert!((p.y - 10.0).abs() < 0.01, "y should be 10, got {}", p.y);
            }
            other => panic!("expected MoveTo, got {other:?}"),
        }
    }

    #[test]
    fn multi_op_transform_applies_left_to_right() {
        // "translate(10,0) scale(2)" = translate·scale, so a point scales
        // first then translates: (5,5) → (10,10) → (20,10). (The reversed
        // order would give (30,10).)
        let svg = r#"<svg viewBox="0 0 100 100">
            <g transform="translate(10,0) scale(2)">
                <rect x="5" y="5" width="1" height="1"/>
            </g>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        match icon.raw_path().commands.first() {
            Some(PathCommand::MoveTo(p)) => {
                assert!(
                    (p.x - 20.0).abs() < 0.01 && (p.y - 10.0).abs() < 0.01,
                    "expected (20,10), got ({},{})",
                    p.x,
                    p.y
                );
            }
            other => panic!("expected MoveTo, got {other:?}"),
        }
    }

    #[test]
    fn rotate_around_center_uses_correct_pivot() {
        // rotate(90 10 10) of (20,10): relative to the pivot (10,0), a 90°
        // (clockwise, y-down) rotation gives (0,10), + pivot → (10,20).
        let svg = r#"<svg viewBox="0 0 100 100">
            <g transform="rotate(90 10 10)">
                <rect x="20" y="10" width="1" height="1"/>
            </g>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        match icon.raw_path().commands.first() {
            Some(PathCommand::MoveTo(p)) => {
                assert!(
                    (p.x - 10.0).abs() < 0.01 && (p.y - 20.0).abs() < 0.01,
                    "expected (10,20), got ({},{})",
                    p.x,
                    p.y
                );
            }
            other => panic!("expected MoveTo, got {other:?}"),
        }
    }

    #[test]
    fn defs_not_rendered() {
        let svg = r#"<svg viewBox="0 0 24 24"><defs><path d="M0 0L24 24"/></defs></svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(icon.is_empty(), "content inside <defs> must not render");
    }

    #[test]
    fn symbol_use_renders_with_offset() {
        // A <symbol> renders only via <use>; the use x/y offsets it.
        let svg = r##"<svg viewBox="0 0 48 48">
            <symbol id="sq"><rect x="0" y="0" width="10" height="10"/></symbol>
            <use href="#sq" x="20" y="5"/>
        </svg>"##;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(
            !icon.raw_path().is_empty(),
            "<use> of a <symbol> must render"
        );
        match icon.raw_path().commands.first() {
            Some(PathCommand::MoveTo(p)) => {
                assert!(
                    (p.x - 20.0).abs() < 0.01 && (p.y - 5.0).abs() < 0.01,
                    "rect should be offset to (20,5), got ({},{})",
                    p.x,
                    p.y
                );
            }
            other => panic!("expected MoveTo, got {other:?}"),
        }
    }

    #[test]
    fn unknown_id_use_is_empty() {
        let svg = r##"<svg viewBox="0 0 24 24"><use href="#missing"/></svg>"##;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(icon.is_empty(), "dangling <use> renders nothing, no error");
    }

    #[test]
    fn use_cycle_terminates() {
        // A symbol that <use>s itself must terminate (depth guard), not overflow.
        let svg = r##"<svg viewBox="0 0 24 24">
            <symbol id="a"><use href="#a"/></symbol>
            <use href="#a"/>
        </svg>"##;
        assert!(
            SvgIcon::parse(svg).is_ok(),
            "self-referential <use> must terminate cleanly"
        );
    }

    #[test]
    fn deeply_nested_groups_ok() {
        let mut svg = String::from(r#"<svg viewBox="0 0 24 24">"#);
        for _ in 0..100 {
            svg.push_str("<g>");
        }
        svg.push_str(r#"<rect x="0" y="0" width="1" height="1"/>"#);
        for _ in 0..100 {
            svg.push_str("</g>");
        }
        svg.push_str("</svg>");
        let icon = SvgIcon::parse(&svg).unwrap();
        assert!(!icon.is_empty(), "100 nested groups (< depth bound) render");
    }

    // ── CSS <style> / class, display / visibility ─────────────────────

    #[test]
    fn style_block_drives_stroke() {
        // A CSS class turns a path into a line icon (fill:none + stroke).
        // Without CSS support it would fall back to filled-black (a blob).
        let svg = r#"<svg viewBox="0 0 24 24">
            <style>.icon{fill:none;stroke:#000;stroke-width:2}</style>
            <path class="icon" d="M2 12L22 12"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(
            icon.raw_path().is_empty(),
            "CSS fill:none must suppress fill"
        );
        assert_eq!(icon.strokes().len(), 1, "CSS stroke must apply");
        assert!((icon.strokes()[0].width - 2.0).abs() < 0.01);
    }

    #[test]
    fn css_at_rules_dont_drop_real_rules() {
        // A stray @charset / @media (common in Illustrator/Inkscape exports)
        // must not poison the following selector or drop the rest of the
        // sheet — the real `.icon` rule must still apply.
        let svg = r#"<svg viewBox="0 0 24 24">
            <style>@charset "UTF-8"; @media print { .x{fill:red} } .icon{fill:none;stroke:#000;stroke-width:2}</style>
            <path class="icon" d="M2 12L22 12"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(
            icon.raw_path().is_empty(),
            ".icon fill:none must apply despite the @-rules"
        );
        assert_eq!(icon.strokes().len(), 1);
        assert!((icon.strokes()[0].width - 2.0).abs() < 0.01);
    }

    #[test]
    fn tag_selector_fill() {
        let svg = r#"<svg viewBox="0 0 24 24">
            <style>path{fill:none;stroke:#000;stroke-width:1}</style>
            <path d="M0 0L24 24"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(icon.raw_path().is_empty(), "tag selector fill:none applies");
        assert_eq!(icon.strokes().len(), 1);
    }

    #[test]
    fn class_multiple_values() {
        let svg = r#"<svg viewBox="0 0 24 24">
            <style>.a{fill:none} .b{stroke:#000;stroke-width:3}</style>
            <path class="a b" d="M0 0L24 0"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(icon.raw_path().is_empty(), ".a fill:none applies");
        assert_eq!(icon.strokes().len(), 1, ".b stroke applies");
        assert!((icon.strokes()[0].width - 3.0).abs() < 0.01);
    }

    #[test]
    fn css_class_beats_presentation_attr() {
        let svg = r#"<svg viewBox="0 0 24 24">
            <style>.x{fill:none}</style>
            <rect x="0" y="0" width="10" height="10" fill="black" class="x"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(
            icon.raw_path().is_empty(),
            "class fill:none must beat presentation fill=black"
        );
    }

    #[test]
    fn inline_style_beats_css_class() {
        let svg = r#"<svg viewBox="0 0 24 24">
            <style>.x{fill:none}</style>
            <rect x="0" y="0" width="10" height="10" class="x" style="fill:black"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(
            !icon.raw_path().is_empty(),
            "inline fill:black must beat class fill:none"
        );
    }

    #[test]
    fn display_none_prunes() {
        let svg = r#"<svg viewBox="0 0 24 24">
            <rect x="0" y="0" width="10" height="10" display="none"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(icon.is_empty(), "display=none element must not render");
    }

    #[test]
    fn display_none_in_style_prunes_subtree() {
        let svg = r#"<svg viewBox="0 0 24 24">
            <g style="display:none"><rect x="0" y="0" width="10" height="10"/></g>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(icon.is_empty(), "display:none group prunes its subtree");
    }

    #[test]
    fn visibility_hidden_suppresses_shape() {
        let svg = r#"<svg viewBox="0 0 24 24">
            <rect x="0" y="0" width="10" height="10" visibility="hidden"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(icon.is_empty(), "visibility=hidden suppresses the shape");
    }

    #[test]
    fn visibility_visible_child_overrides_hidden_parent() {
        // visibility is inherited, but a child can override back to visible
        // (and the walk keeps recursing through a hidden group).
        let svg = r#"<svg viewBox="0 0 24 24">
            <g visibility="hidden">
                <rect x="0" y="0" width="10" height="10" visibility="visible"/>
            </g>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(
            !icon.is_empty(),
            "a visible child of a hidden group must render"
        );
    }

    // ── fill-rule, opacity, dasharray ─────────────────────────────────

    #[test]
    fn fill_rule_winding_stays_in_path() {
        let svg = r#"<svg viewBox="0 0 24 24"><path d="M0 0L24 0L24 24Z"/></svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(!icon.raw_path().is_empty());
        assert!(
            icon.extra_fills().is_empty(),
            "default winding fill stays in the main path"
        );
    }

    #[test]
    fn fill_rule_evenodd_in_extra_fills() {
        let svg = r#"<svg viewBox="0 0 24 24">
            <path fill-rule="evenodd" d="M0 0L24 0L24 24Z"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(
            icon.raw_path().is_empty(),
            "evenodd fill must go to extra_fills, not the main winding path"
        );
        assert_eq!(icon.extra_fills().len(), 1);
        assert_eq!(icon.extra_fills()[0].fill_rule, FillRule::EvenOdd);
    }

    #[test]
    fn opacity_below_1_goes_to_extra_fills() {
        let svg = r#"<svg viewBox="0 0 24 24">
            <rect x="0" y="0" width="10" height="10" opacity="0.5"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(
            icon.raw_path().is_empty(),
            "a translucent fill must go to extra_fills"
        );
        assert_eq!(icon.extra_fills().len(), 1);
        assert!((icon.extra_fills()[0].opacity - 0.5).abs() < 0.01);
    }

    #[test]
    fn stroke_dasharray_sets_dash_pattern() {
        let svg = r#"<svg viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-dasharray="5 3">
            <line x1="0" y1="0" x2="24" y2="0"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert_eq!(icon.strokes().len(), 1);
        let dash = icon.strokes()[0].dash.as_ref().expect("dash present");
        assert_eq!(dash.0, vec![5.0, 3.0]);
    }

    #[test]
    fn stroke_dasharray_odd_length_doubles() {
        let svg = r#"<svg viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-dasharray="4">
            <line x1="0" y1="0" x2="24" y2="0"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        let dash = icon.strokes()[0].dash.as_ref().expect("dash present");
        assert_eq!(
            dash.0,
            vec![4.0, 4.0],
            "an odd-length dash array is doubled per spec"
        );
    }

    // ── Fidelity: miterlimit, preserveAspectRatio, rect rx≠ry, units ──

    #[test]
    fn miter_limit_parses() {
        let svg = r#"<svg viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-miterlimit="10">
            <path d="M0 0L10 10L20 0"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert_eq!(icon.strokes().len(), 1);
        assert!((icon.strokes()[0].miter_limit - 10.0).abs() < 0.01);
    }

    #[test]
    fn rect_rx_ry_distinct_uses_elliptical_corners() {
        let svg = r#"<svg viewBox="0 0 24 24">
            <rect x="0" y="0" width="20" height="10" rx="4" ry="2"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        let cmds = &icon.raw_path().commands;
        let arcs = cmds
            .iter()
            .filter(|c| matches!(c, PathCommand::ArcTo { .. }))
            .count();
        assert_eq!(arcs, 4, "four elliptical corner arcs");
        // First corner arc (top-right) must be a 2rx × 2ry = 8 × 4 ellipse.
        let arc = cmds
            .iter()
            .find_map(|c| match c {
                PathCommand::ArcTo { rect, .. } => Some(*rect),
                _ => None,
            })
            .unwrap();
        assert!(
            (arc.width - 8.0).abs() < 0.01 && (arc.height - 4.0).abs() < 0.01,
            "corner ellipse should be 8×4, got {}×{}",
            arc.width,
            arc.height
        );
    }

    #[test]
    fn aspect_ratio_none_stretches() {
        // 20×10 viewBox + none → stretch to fill: sx=2, sy=4, (10,5)→(20,20).
        let svg = r#"<svg viewBox="0 0 20 10" preserveAspectRatio="none">
            <rect x="10" y="5" width="1" height="1"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        let path = icon.to_path_in_rect(Rect::new(0.0, 0.0, 40.0, 40.0));
        match path.commands.first() {
            Some(PathCommand::MoveTo(p)) => {
                assert!(
                    (p.x - 20.0).abs() < 0.01 && (p.y - 20.0).abs() < 0.01,
                    "stretched to ({},{})",
                    p.x,
                    p.y
                );
            }
            other => panic!("expected MoveTo, got {other:?}"),
        }
    }

    #[test]
    fn aspect_ratio_xminymin_aligns_to_corner() {
        // 20×10 meet into 40×40 → scale 2 (40×20), aligned top-left.
        let svg = r#"<svg viewBox="0 0 20 10" preserveAspectRatio="xMinYMin meet">
            <rect x="0" y="0" width="1" height="1"/>
        </svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        let path = icon.to_path_in_rect(Rect::new(0.0, 0.0, 40.0, 40.0));
        match path.commands.first() {
            Some(PathCommand::MoveTo(p)) => {
                assert!(
                    p.x.abs() < 0.01 && p.y.abs() < 0.01,
                    "top-left aligned, got ({},{})",
                    p.x,
                    p.y
                );
            }
            other => panic!("expected MoveTo, got {other:?}"),
        }
    }

    #[test]
    fn length_with_pt_unit_parses() {
        let svg = r#"<svg width="24pt" height="24pt"><rect width="10" height="10"/></svg>"#;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!((icon.width() - 24.0).abs() < 0.01);
    }

    #[test]
    fn percent_width_no_viewbox_is_clean_error() {
        // No viewBox + percentage dimensions can't establish a coordinate
        // space → clean Err, no panic.
        let svg = r#"<svg width="100%" height="100%"><rect width="10" height="10"/></svg>"#;
        assert!(SvgIcon::parse(svg).is_err());
    }

    // ── Full color ──────────────────────────────────────────────────────────

    const RECT: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 100.0,
    };
    /// The tint a widget would supply. Opaque black, so a test can tell "the
    /// icon's own color" from "the widget's color" at a glance.
    const TINT: Color = Color::BLACK;

    fn solid_paints(icon: &SvgIcon, tint: Color) -> Vec<Color> {
        icon.draw_ops_in_rect(RECT, tint)
            .into_iter()
            .map(|op| match op {
                SvgDrawOp::Fill { paint, .. } | SvgDrawOp::Stroke { paint, .. } => match paint {
                    Paint::Solid(c) => c,
                    other => panic!("expected a solid paint, got {other:?}"),
                },
            })
            .collect()
    }

    /// Each shape keeps the color it was authored with — the whole point.
    #[test]
    fn shapes_keep_their_own_colors() {
        let svg = r##"<svg viewBox="0 0 100 100">
            <rect width="50" height="50" fill="#ff0000"/>
            <circle cx="70" cy="70" r="20" fill="rgb(0, 255, 0)"/>
            <rect x="0" y="60" width="10" height="10" fill="blue"/>
        </svg>"##;
        let icon = SvgIcon::parse(svg).unwrap();
        let colors = solid_paints(&icon, TINT);
        assert_eq!(colors.len(), 3);
        assert_eq!(colors[0].to_array(), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(colors[1].to_array(), [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(colors[2].to_array(), [0.0, 0.0, 1.0, 1.0]);
        assert!(!icon.is_monochrome());
    }

    /// **Document order is preserved.** The tinted representation merges every
    /// fill into one path and paints fills before strokes — invisible in one
    /// color, and wrong here: the red square is authored *over* the blue one, and
    /// must be drawn second or the overlap comes out blue.
    #[test]
    fn ops_keep_document_order() {
        let svg = r##"<svg viewBox="0 0 100 100">
            <rect width="80" height="80" fill="blue"/>
            <rect width="40" height="40" fill="red"/>
        </svg>"##;
        let icon = SvgIcon::parse(svg).unwrap();
        let colors = solid_paints(&icon, TINT);
        assert_eq!(
            colors[0].to_array(),
            [0.0, 0.0, 1.0, 1.0],
            "the blue square is authored first, so it must paint first"
        );
        assert_eq!(colors[1].to_array(), [1.0, 0.0, 0.0, 1.0]);
    }

    /// `currentColor` — and an unresolvable paint — take the widget's tint, so
    /// artwork can carry one themed accent among fixed colors.
    #[test]
    fn current_color_and_dangling_refs_take_the_tint() {
        let svg = r##"<svg viewBox="0 0 100 100">
            <rect width="10" height="10" fill="currentColor"/>
            <rect width="10" height="10" fill="url(#nope)"/>
            <rect width="10" height="10" fill="#123456"/>
        </svg>"##;
        let icon = SvgIcon::parse(svg).unwrap();
        let tint = Color::new(0.2, 0.4, 0.6, 1.0);
        let colors = solid_paints(&icon, tint);
        assert_eq!(colors[0].to_array(), tint.to_array());
        assert_eq!(
            colors[1].to_array(),
            tint.to_array(),
            "a dangling url(#…) must stay visible in the tint, not vanish"
        );
        assert_ne!(colors[2].to_array(), tint.to_array());
    }

    /// A `url(#…)` may carry a fallback color for exactly this case.
    #[test]
    fn a_dangling_ref_prefers_its_authored_fallback() {
        let svg = r##"<svg viewBox="0 0 100 100">
            <rect width="10" height="10" fill="url(#nope) lime"/>
        </svg>"##;
        let icon = SvgIcon::parse(svg).unwrap();
        assert_eq!(
            solid_paints(&icon, TINT)[0].to_array(),
            [0.0, 1.0, 0.0, 1.0]
        );
    }

    /// Colors inherit through groups, and the cascade order still holds
    /// (presentation attribute < CSS < inline style).
    #[test]
    fn color_inherits_through_groups_and_cascades() {
        let svg = r##"<svg viewBox="0 0 100 100">
            <style>.hot { fill: orange; }</style>
            <g fill="red">
                <rect width="10" height="10"/>
                <rect width="10" height="10" fill="blue"/>
                <rect width="10" height="10" class="hot"/>
                <rect width="10" height="10" class="hot" style="fill: white"/>
            </g>
        </svg>"##;
        let icon = SvgIcon::parse(svg).unwrap();
        let c = solid_paints(&icon, TINT);
        assert_eq!(c[0].to_array(), [1.0, 0.0, 0.0, 1.0], "inherited from <g>");
        assert_eq!(c[1].to_array(), [0.0, 0.0, 1.0, 1.0], "own attribute wins");
        assert_eq!(c[2].to_hex_upper(false), "#FFA500", "CSS rule beats <g>");
        assert_eq!(c[3].to_array(), [1.0, 1.0, 1.0, 1.0], "inline style wins");
    }

    /// An icon with no colors of its own is monochrome, and paints as the tint.
    /// The widget skips the ordered walk entirely for these — most UI glyphs.
    #[test]
    fn a_current_color_icon_is_monochrome() {
        let svg = r##"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="12" cy="12" r="10"/>
        </svg>"##;
        assert!(SvgIcon::parse(svg).unwrap().is_monochrome());
        // A stated color — even black — is a color, and full-color must honour it.
        let colored =
            r##"<svg viewBox="0 0 24 24"><rect width="8" height="8" fill="black"/></svg>"##;
        assert!(!SvgIcon::parse(colored).unwrap().is_monochrome());
    }

    /// Opacity folds into the paint's alpha, and the tint's alpha attenuates the
    /// whole icon — but counts only **once** for a `currentColor` shape, which
    /// already carries it.
    #[test]
    fn opacity_folds_in_without_double_counting_the_tint() {
        let svg = r##"<svg viewBox="0 0 100 100">
            <rect width="10" height="10" fill="red" fill-opacity="0.5"/>
            <rect width="10" height="10" fill="currentColor"/>
        </svg>"##;
        let icon = SvgIcon::parse(svg).unwrap();
        let half_tint = Color::new(1.0, 1.0, 1.0, 0.5);
        let c = solid_paints(&icon, half_tint);
        assert!(
            (c[0].a() - 0.25).abs() < 1e-5,
            "0.5 fill-opacity × 0.5 tint alpha = 0.25, got {}",
            c[0].a()
        );
        assert!(
            (c[1].a() - 0.5).abs() < 1e-5,
            "a currentColor shape carries the tint's alpha once, got {}",
            c[1].a()
        );
    }

    /// A fully transparent shape costs no draw op at all.
    #[test]
    fn invisible_shapes_are_dropped() {
        let svg = r##"<svg viewBox="0 0 100 100">
            <rect width="10" height="10" fill="red" fill-opacity="0"/>
            <rect width="10" height="10" fill="transparent"/>
            <rect width="10" height="10" fill="red"/>
        </svg>"##;
        let icon = SvgIcon::parse(svg).unwrap();
        assert_eq!(icon.draw_ops_in_rect(RECT, TINT).len(), 1);
    }

    /// Fills and strokes are separate ops, and a stroke keeps its style.
    #[test]
    fn a_filled_and_stroked_shape_emits_both_ops_in_order() {
        let svg = r##"<svg viewBox="0 0 100 100">
            <rect width="50" height="50" fill="red" stroke="blue" stroke-width="4"/>
        </svg>"##;
        let icon = SvgIcon::parse(svg).unwrap();
        let ops = icon.draw_ops_in_rect(RECT, TINT);
        assert_eq!(ops.len(), 2);
        match &ops[0] {
            SvgDrawOp::Fill { paint, .. } => {
                assert_eq!(*paint, Paint::Solid(Color::new(1.0, 0.0, 0.0, 1.0)))
            }
            other => panic!("expected the fill first, got {other:?}"),
        }
        match &ops[1] {
            SvgDrawOp::Stroke { style, paint, .. } => {
                assert_eq!(*paint, Paint::Solid(Color::new(0.0, 0.0, 1.0, 1.0)));
                // viewBox 100 → rect 100: scale 1, so the width passes through.
                assert!((style.width - 4.0).abs() < 1e-5);
            }
            other => panic!("expected the stroke second, got {other:?}"),
        }
    }

    // ── Gradients ───────────────────────────────────────────────────────────

    fn first_paint(svg: &str, tint: Color) -> Paint {
        let icon = SvgIcon::parse(svg).unwrap();
        match icon.draw_ops_in_rect(RECT, tint).remove(0) {
            SvgDrawOp::Fill { paint, .. } | SvgDrawOp::Stroke { paint, .. } => paint,
        }
    }

    /// The default `objectBoundingBox` units make a gradient a function of the
    /// *shape's* box: `x1=0 → x2=1` spans the rect it lands on, wherever that is.
    /// Coordinates come back path-bounds-local (what the canvas normalizes
    /// against), so a 50×50 rect at (25, 25) gets a ramp from (0,0) to (50,0).
    #[test]
    fn a_linear_gradient_binds_to_the_shape_bounding_box() {
        let svg = r##"<svg viewBox="0 0 100 100">
            <defs>
              <linearGradient id="g">
                <stop offset="0" stop-color="red"/>
                <stop offset="1" stop-color="blue"/>
              </linearGradient>
            </defs>
            <rect x="25" y="25" width="50" height="50" fill="url(#g)"/>
        </svg>"##;
        match first_paint(svg, TINT) {
            Paint::LinearGradient { start, end, stops } => {
                assert!(start.x.abs() < 1e-4 && start.y.abs() < 1e-4, "{start:?}");
                assert!((end.x - 50.0).abs() < 1e-4 && end.y.abs() < 1e-4, "{end:?}");
                assert_eq!(stops.len(), 2);
                assert_eq!(stops[0].color.to_array(), [1.0, 0.0, 0.0, 1.0]);
                assert_eq!(stops[1].color.to_array(), [0.0, 0.0, 1.0, 1.0]);
            }
            other => panic!("expected a linear gradient, got {other:?}"),
        }
    }

    /// `userSpaceOnUse` coordinates are in the user space instead — the same
    /// declaration must NOT be re-scaled to the shape.
    #[test]
    fn user_space_gradient_coordinates_are_absolute() {
        let svg = r##"<svg viewBox="0 0 100 100">
            <linearGradient id="g" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="100" y2="0">
              <stop offset="0" stop-color="red"/>
              <stop offset="1" stop-color="blue"/>
            </linearGradient>
            <rect x="20" y="20" width="20" height="20" fill="url(#g)"/>
        </svg>"##;
        match first_paint(svg, TINT) {
            // The ramp spans the whole 100-wide viewBox; the shape sits at x=20,
            // so path-bounds-local it runs from -20 to +80.
            Paint::LinearGradient { start, end, .. } => {
                assert!((start.x + 20.0).abs() < 1e-4, "start {start:?}");
                assert!((end.x - 80.0).abs() < 1e-4, "end {end:?}");
            }
            other => panic!("expected a linear gradient, got {other:?}"),
        }
    }

    /// **`href` inheritance.** The idiomatic "one ramp, several directions"
    /// authoring: the referencing gradient declares no stops of its own. Dropping
    /// `href` doesn't render it slightly wrong — it renders nothing.
    #[test]
    fn a_gradient_inherits_stops_through_href() {
        let svg = r##"<svg viewBox="0 0 100 100">
            <defs>
              <linearGradient id="ramp">
                <stop offset="0" stop-color="red"/>
                <stop offset="1" stop-color="lime"/>
              </linearGradient>
              <linearGradient id="down" xlink:href="#ramp" x1="0" y1="0" x2="0" y2="1"/>
            </defs>
            <rect width="100" height="100" fill="url(#down)"/>
        </svg>"##;
        match first_paint(svg, TINT) {
            Paint::LinearGradient { start, end, stops } => {
                assert_eq!(stops.len(), 2, "stops must come from the referenced ramp");
                assert_eq!(stops[1].color.to_array(), [0.0, 1.0, 0.0, 1.0]);
                // …but the *geometry* is this gradient's own: top-to-bottom.
                assert!(start.y.abs() < 1e-4 && (end.y - 100.0).abs() < 1e-4);
                assert!(end.x.abs() < 1e-4, "direction must be vertical, {end:?}");
            }
            other => panic!("expected a linear gradient, got {other:?}"),
        }
    }

    /// `gradientTransform` composes inside the bounding-box mapping.
    #[test]
    fn gradient_transform_applies() {
        let svg = r##"<svg viewBox="0 0 100 100">
            <linearGradient id="g" gradientTransform="rotate(90)">
              <stop offset="0" stop-color="red"/>
              <stop offset="1" stop-color="blue"/>
            </linearGradient>
            <rect width="100" height="100" fill="url(#g)"/>
        </svg>"##;
        match first_paint(svg, TINT) {
            // The default ramp runs +x; rotated 90° it runs +y.
            Paint::LinearGradient { start, end, .. } => {
                assert!(start.x.abs() < 1e-3 && start.y.abs() < 1e-3, "{start:?}");
                assert!(
                    end.x.abs() < 1e-3 && (end.y - 100.0).abs() < 1e-3,
                    "rotate(90) should turn the ramp downward, got {end:?}"
                );
            }
            other => panic!("expected a linear gradient, got {other:?}"),
        }
    }

    /// Radial defaults (`cx=cy=r=50%`) centre the gradient on the shape.
    #[test]
    fn a_radial_gradient_defaults_to_the_shape_centre() {
        let svg = r##"<svg viewBox="0 0 100 100">
            <radialGradient id="g">
              <stop offset="0" stop-color="white"/>
              <stop offset="1" stop-color="black"/>
            </radialGradient>
            <rect width="80" height="80" fill="url(#g)"/>
        </svg>"##;
        match first_paint(svg, TINT) {
            Paint::RadialGradient {
                center,
                radius,
                stops,
            } => {
                assert!((center.x - 40.0).abs() < 1e-4 && (center.y - 40.0).abs() < 1e-4);
                assert!((radius - 40.0).abs() < 1e-4, "radius {radius}");
                assert_eq!(stops.len(), 2);
            }
            other => panic!("expected a radial gradient, got {other:?}"),
        }
    }

    /// `stop-opacity` folds into the stop's alpha, and a `currentColor` stop
    /// resolves to the tint — baking it black at parse time would silently
    /// unthemed the ramp.
    #[test]
    fn stops_carry_opacity_and_current_color() {
        let svg = r##"<svg viewBox="0 0 100 100">
            <linearGradient id="g">
              <stop offset="0" stop-color="red" stop-opacity="0.5"/>
              <stop offset="1" stop-color="currentColor"/>
            </linearGradient>
            <rect width="100" height="100" fill="url(#g)"/>
        </svg>"##;
        let tint = Color::new(0.0, 1.0, 0.0, 1.0);
        match first_paint(svg, tint) {
            Paint::LinearGradient { stops, .. } => {
                assert!((stops[0].color.a() - 0.5).abs() < 1e-5);
                assert_eq!(
                    stops[1].color.to_array(),
                    tint.to_array(),
                    "a currentColor stop must take the tint"
                );
            }
            other => panic!("expected a linear gradient, got {other:?}"),
        }
    }

    /// A gradient with a single stop is a solid color — but the ramp still needs
    /// two ends, or the shader has nothing to interpolate between.
    #[test]
    fn a_one_stop_gradient_becomes_a_flat_ramp() {
        let svg = r##"<svg viewBox="0 0 100 100">
            <linearGradient id="g"><stop offset="0.5" stop-color="red"/></linearGradient>
            <rect width="100" height="100" fill="url(#g)"/>
        </svg>"##;
        match first_paint(svg, TINT) {
            Paint::LinearGradient { stops, .. } => {
                assert_eq!(stops.len(), 2);
                assert_eq!(stops[0].offset, 0.0);
                assert_eq!(stops[1].offset, 1.0);
                assert_eq!(stops[0].color.to_array(), [1.0, 0.0, 0.0, 1.0]);
            }
            other => panic!("expected a linear gradient, got {other:?}"),
        }
    }

    /// A gradient with no stops paints nothing, so the shape falls back to the
    /// tint rather than disappearing.
    #[test]
    fn a_stopless_gradient_falls_back_to_the_tint() {
        let svg = r##"<svg viewBox="0 0 100 100">
            <linearGradient id="g"/>
            <rect width="100" height="100" fill="url(#g)"/>
        </svg>"##;
        let tint = Color::new(0.1, 0.2, 0.3, 1.0);
        assert_eq!(first_paint(svg, tint), Paint::Solid(tint));
    }

    /// An `href` cycle is malformed, but it must terminate rather than hang.
    #[test]
    fn an_href_cycle_terminates() {
        let svg = r##"<svg viewBox="0 0 100 100">
            <linearGradient id="a" xlink:href="#b"/>
            <linearGradient id="b" xlink:href="#a"/>
            <rect width="100" height="100" fill="url(#a)"/>
        </svg>"##;
        // No stops resolve out of the cycle → the shape stays visible as tint.
        assert_eq!(first_paint(svg, TINT), Paint::Solid(TINT));
    }

    /// The gradient scales with the icon: fitted into a half-size rect, the
    /// ramp's endpoints halve with the geometry.
    #[test]
    fn gradient_geometry_follows_the_fit() {
        let svg = r##"<svg viewBox="0 0 100 100">
            <linearGradient id="g">
              <stop offset="0" stop-color="red"/><stop offset="1" stop-color="blue"/>
            </linearGradient>
            <rect width="100" height="100" fill="url(#g)"/>
        </svg>"##;
        let icon = SvgIcon::parse(svg).unwrap();
        let ops = icon.draw_ops_in_rect(Rect::new(0.0, 0.0, 50.0, 50.0), TINT);
        match &ops[0] {
            SvgDrawOp::Fill {
                paint: Paint::LinearGradient { end, .. },
                ..
            } => assert!((end.x - 50.0).abs() < 1e-4, "end {end:?}"),
            other => panic!("expected a linear gradient fill, got {other:?}"),
        }
    }

    /// The tinted representation is untouched by all of the above: a colored
    /// document still merges into one silhouette for `IconMode::Tintable`.
    #[test]
    fn the_tinted_representation_still_merges_colored_shapes() {
        let svg = r##"<svg viewBox="0 0 100 100">
            <rect width="50" height="50" fill="red"/>
            <rect x="50" y="50" width="50" height="50" fill="blue"/>
        </svg>"##;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(
            icon.extra_fills().is_empty(),
            "two opaque winding fills still merge into the single hot path"
        );
        assert!(!icon.raw_path().is_empty());
        assert_eq!(icon.ops().len(), 2, "…while full-color keeps them apart");
    }

    /// An SVG that only wraps an embedded raster (`<image>`) is not vector art
    /// and renders as nothing — the parser must not pretend otherwise.
    #[test]
    fn an_image_only_svg_has_no_geometry() {
        let svg = r##"<svg viewBox="0 0 512 512">
            <image x="0" y="0" width="512" height="512" xlink:href="data:image/png;base64,iVBOR"/>
        </svg>"##;
        let icon = SvgIcon::parse(svg).unwrap();
        assert!(icon.is_empty());
        assert!(icon.draw_ops_in_rect(RECT, TINT).is_empty());
    }
}
