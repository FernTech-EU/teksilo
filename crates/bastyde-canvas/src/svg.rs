// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! SVG parsing: load SVG icons as [`Path`] geometry for rendering.
//!
//! The main entry point is [`SvgIcon::parse`], which takes an SVG string
//! and produces geometry in viewBox coordinates. Colors are stripped —
//! the rendering widget controls the paint color, enabling theme-aware
//! icon tinting.
//!
//! Both *filled* and *stroked* (line-style) icons are supported. The
//! parser tracks each element's `fill` / `stroke` / `stroke-width` /
//! `stroke-linecap` / `stroke-linejoin` presentation attributes (and
//! their inline-`style` equivalents), inheriting them through `<g>`
//! groups and the `<svg>` root the way SVG does. Filled geometry merges
//! into one [`Path`] ([`SvgIcon::raw_path`]); stroked geometry is kept
//! separately as [`SvgStroke`]s carrying their viewBox-space width and
//! cap/join, so the line-style convention (`fill="none"
//! stroke="currentColor"`) renders as outlines instead of solid blobs.

pub(crate) mod path_parser;

use crate::geometry::{Point, Rect, Transform2D};
use crate::paint::{LineCap, LineJoin, StrokeStyle};
use crate::path::Path;
use crate::xml::{XmlElement, parse_dom};

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
}

/// A parsed SVG icon: geometry + viewBox, ready to be scaled and
/// rendered. Original colors are stripped; filled and stroked geometry
/// are kept separately so line-style icons render as outlines.
#[derive(Debug, Clone)]
pub struct SvgIcon {
    /// Merged *filled* path in viewBox coordinates.
    path: Path,
    /// *Stroked* sub-paths in viewBox coordinates, grouped by stroke
    /// style. Empty for the common all-filled icon.
    strokes: Vec<SvgStroke>,
    /// The SVG viewBox (defines the coordinate space).
    view_box: Rect,
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

        let mut builder = SvgBuilder::default();
        walk_element(
            svg_el,
            &Transform2D::IDENTITY,
            SvgPaintState::default(),
            &mut builder,
        )?;

        Ok(SvgIcon {
            path: builder.fill,
            strokes: builder.strokes,
            view_box,
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

    /// Produce the *stroked* sub-paths scaled to fit within `rect`,
    /// each paired with a ready-to-render [`StrokeStyle`] whose width is
    /// scaled into display space. Empty for the common filled-only icon.
    ///
    /// Pair with [`to_path_in_rect`](Self::to_path_in_rect): fill the
    /// returned path, then stroke each of these — an icon may carry both
    /// (a filled shape with a stroked border).
    pub fn stroked_paths_in_rect(&self, rect: Rect) -> Vec<(Path, StrokeStyle)> {
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
                (s.path.transformed(&transform), style)
            })
            .collect()
    }

    /// The aspect-ratio-preserving fit transform mapping viewBox
    /// coordinates into `rect` (scale, then center), together with the
    /// uniform scale factor applied. `None` if the viewBox is degenerate.
    fn fit_transform(&self, rect: Rect) -> Option<(Transform2D, f32)> {
        if self.view_box.width <= 0.0 || self.view_box.height <= 0.0 {
            return None;
        }
        let scale = (rect.width / self.view_box.width).min(rect.height / self.view_box.height);
        let scaled_w = self.view_box.width * scale;
        let scaled_h = self.view_box.height * scale;
        let offset_x = rect.x + (rect.width - scaled_w) / 2.0;
        let offset_y = rect.y + (rect.height - scaled_h) / 2.0;

        // Scale viewBox coordinates first, then translate to target position.
        let transform = Transform2D::scale(scale, scale).then(&Transform2D::translate(
            offset_x - self.view_box.x * scale,
            offset_y - self.view_box.y * scale,
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

    /// Whether this icon carries any geometry at all (filled or stroked).
    pub fn is_empty(&self) -> bool {
        self.path.is_empty() && self.strokes.is_empty()
    }

    /// Access the viewBox.
    pub fn view_box(&self) -> Rect {
        self.view_box
    }
}

// --- Internal helpers ---

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

/// Parse a length value, stripping unit suffixes like "px".
fn parse_length(s: &str) -> Option<f32> {
    let s = s.trim().trim_end_matches("px").trim_end_matches("pt");
    s.parse::<f32>().ok()
}

/// Accumulates parsed geometry: one merged fill path plus stroked
/// sub-paths grouped by stroke style.
#[derive(Default)]
struct SvgBuilder {
    fill: Path,
    strokes: Vec<SvgStroke>,
}

impl SvgBuilder {
    /// Add a stroked sub-path, merging it into an existing group that
    /// shares the same width / cap / join so the common "one stroke
    /// style for the whole icon" case stays a single entry (and one
    /// rasterized atlas tile). Strokes are per-contour, so appending
    /// extra `MoveTo`-started contours is equivalent to stroking each
    /// separately.
    fn push_stroke(&mut self, path: Path, width: f32, line_cap: LineCap, line_join: LineJoin) {
        const EPS: f32 = 1e-4;
        if let Some(group) = self.strokes.iter_mut().find(|s| {
            (s.width - width).abs() < EPS && s.line_cap == line_cap && s.line_join == line_join
        }) {
            group.path.append(&path);
        } else {
            self.strokes.push(SvgStroke {
                path,
                width,
                line_cap,
                line_join,
            });
        }
    }
}

/// The resolved paint state for an element — what SVG's `fill` /
/// `stroke` presentation attributes would compute to, with colors
/// reduced to "is this paint active?" booleans (colors are stripped for
/// theme tinting). Inherited down the element tree like real SVG.
#[derive(Debug, Clone, Copy)]
struct SvgPaintState {
    /// Whether the shape's interior is filled (`fill` != `none`).
    fill: bool,
    /// Whether the shape's outline is stroked (`stroke` != `none`).
    stroke: bool,
    /// Stroke width in the element's local coordinate space.
    stroke_width: f32,
    line_cap: LineCap,
    line_join: LineJoin,
}

impl Default for SvgPaintState {
    fn default() -> Self {
        // SVG initial values: fill = black (painted), stroke = none,
        // stroke-width = 1, butt caps, miter joins.
        Self {
            fill: true,
            stroke: false,
            stroke_width: 1.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
        }
    }
}

fn walk_element(
    node: &XmlElement,
    parent_transform: &Transform2D,
    parent_paint: SvgPaintState,
    builder: &mut SvgBuilder,
) -> Result<(), SvgParseError> {
    let transform = if let Some(t_attr) = node.attribute("transform") {
        let local = parse_transform(t_attr)?;
        parent_transform.then(&local)
    } else {
        *parent_transform
    };

    let paint = resolve_paint_state(node, parent_paint);

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

    if let Some(local) = shape {
        let world = if transform != Transform2D::IDENTITY {
            local.transformed(&transform)
        } else {
            local
        };
        if paint.fill {
            builder.fill.append(&world);
        }
        if paint.stroke && paint.stroke_width > 0.0 {
            // The width is in local space; bake it into viewBox space by
            // the cumulative transform's scale so it tracks group scaling.
            let width = paint.stroke_width * transform.geometric_scale();
            builder.push_stroke(world, width, paint.line_cap, paint.line_join);
        }
    }

    // Recurse into children (for <g>, <svg>, <defs>, etc.), passing the
    // resolved transform and paint state down so they inherit.
    for child in node.children() {
        walk_element(child, &transform, paint, builder)?;
    }

    Ok(())
}

/// Apply an element's paint-related presentation attributes (and inline
/// `style`) onto the inherited parent state. Inline `style` wins over
/// presentation attributes, both win over inheritance — matching SVG's
/// cascade for these properties.
fn resolve_paint_state(node: &XmlElement, parent: SvgPaintState) -> SvgPaintState {
    let mut st = parent;

    // Presentation attributes.
    if let Some(v) = node.attribute("fill") {
        st.fill = !is_none_paint(v);
    }
    if let Some(v) = node.attribute("stroke") {
        st.stroke = !is_none_paint(v);
    }
    if let Some(w) = node.attribute("stroke-width").and_then(parse_length) {
        st.stroke_width = w;
    }
    if let Some(v) = node.attribute("stroke-linecap") {
        st.line_cap = parse_line_cap(v);
    }
    if let Some(v) = node.attribute("stroke-linejoin") {
        st.line_join = parse_line_join(v);
    }

    // Inline style declarations override presentation attributes.
    if let Some(style) = node.attribute("style") {
        for (key, value) in parse_inline_style(style) {
            match key {
                "fill" => st.fill = !is_none_paint(value),
                "stroke" => st.stroke = !is_none_paint(value),
                "stroke-width" => {
                    if let Some(w) = parse_length(value) {
                        st.stroke_width = w;
                    }
                }
                "stroke-linecap" => st.line_cap = parse_line_cap(value),
                "stroke-linejoin" => st.line_join = parse_line_join(value),
                _ => {}
            }
        }
    }

    st
}

/// Whether a `fill` / `stroke` value means "no paint". Colors are
/// stripped, so any value other than `none` (case-insensitive) counts as
/// painted.
fn is_none_paint(value: &str) -> bool {
    value.trim().eq_ignore_ascii_case("none")
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

/// Split an inline `style="a:b;c:d"` attribute into `(property, value)`
/// pairs, trimmed. Declarations without a `:` are skipped.
fn parse_inline_style(style: &str) -> impl Iterator<Item = (&str, &str)> {
    style.split(';').filter_map(|decl| {
        let (key, value) = decl.split_once(':')?;
        Some((key.trim(), value.trim()))
    })
}

// --- Shape element parsers ---

fn parse_rect_element(node: &XmlElement) -> Option<Path> {
    let x = attr_f32(node, "x").unwrap_or(0.0);
    let y = attr_f32(node, "y").unwrap_or(0.0);
    let w = attr_f32(node, "width")?;
    let h = attr_f32(node, "height")?;
    let rx = attr_f32(node, "rx").unwrap_or(0.0);
    let ry = attr_f32(node, "ry").unwrap_or(rx);
    if rx > 0.0 || ry > 0.0 {
        let r = rx.max(ry);
        Some(Path::rounded_rect(
            Rect::new(x, y, w, h),
            bastyde_tokens::CornerRadius::uniform(r),
        ))
    } else {
        Some(Path::rect(Rect::new(x, y, w, h)))
    }
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

        if let Some(rest) = remaining.strip_prefix("translate") {
            let (args, rest) = parse_transform_args(rest)?;
            let tx = args.first().copied().unwrap_or(0.0);
            let ty = args.get(1).copied().unwrap_or(0.0);
            result = result.then(&Transform2D::translate(tx, ty));
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("scale") {
            let (args, rest) = parse_transform_args(rest)?;
            let sx = args.first().copied().unwrap_or(1.0);
            let sy = args.get(1).copied().unwrap_or(sx);
            result = result.then(&Transform2D::scale(sx, sy));
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("rotate") {
            let (args, rest) = parse_transform_args(rest)?;
            let angle_deg = args.first().copied().unwrap_or(0.0);
            if args.len() >= 3 {
                let cx = args[1];
                let cy = args[2];
                result = result
                    .then(&Transform2D::translate(cx, cy))
                    .then(&Transform2D::rotate(angle_deg.to_radians()))
                    .then(&Transform2D::translate(-cx, -cy));
            } else {
                result = result.then(&Transform2D::rotate(angle_deg.to_radians()));
            }
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("matrix") {
            let (args, rest) = parse_transform_args(rest)?;
            if args.len() != 6 {
                return Err(SvgParseError::InvalidTransform(
                    "matrix requires 6 values".into(),
                ));
            }
            let t = Transform2D {
                m: [args[0], args[1], args[2], args[3], args[4], args[5]],
            };
            result = result.then(&t);
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("skewX") {
            let (args, rest) = parse_transform_args(rest)?;
            let angle = args.first().copied().unwrap_or(0.0).to_radians();
            let t = Transform2D {
                m: [1.0, 0.0, angle.tan(), 1.0, 0.0, 0.0],
            };
            result = result.then(&t);
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("skewY") {
            let (args, rest) = parse_transform_args(rest)?;
            let angle = args.first().copied().unwrap_or(0.0).to_radians();
            let t = Transform2D {
                m: [1.0, angle.tan(), 0.0, 1.0, 0.0, 0.0],
            };
            result = result.then(&t);
            remaining = rest;
        } else {
            return Err(SvgParseError::InvalidTransform(format!(
                "unrecognized transform: {remaining}"
            )));
        }

        // Skip optional comma/whitespace between transforms
        remaining = remaining.trim_start();
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
}
