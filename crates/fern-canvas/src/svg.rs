//! SVG parsing: load SVG icons as [`Path`] geometry for rendering.
//!
//! The main entry point is [`SvgIcon::parse`], which takes an SVG string
//! and produces a merged [`Path`] in viewBox coordinates. Colors in the
//! SVG are stripped — the rendering widget controls the fill color,
//! enabling theme-aware icon tinting.

pub(crate) mod path_parser;

use crate::geometry::{Point, Rect, Transform2D};
use crate::path::Path;

/// Error type for SVG parsing failures.
#[derive(Debug, Clone, PartialEq)]
pub enum SvgParseError {
    /// XML is malformed.
    XmlError(String),
    /// No `<svg>` root element found.
    MissingSvgElement,
    /// viewBox attribute is missing or invalid.
    InvalidViewBox(String),
    /// Path data string (`d` attribute) is malformed.
    InvalidPathData { detail: String, position: usize },
    /// A `transform` attribute could not be parsed.
    InvalidTransform(String),
}

impl std::fmt::Display for SvgParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::XmlError(e) => write!(f, "SVG XML error: {e}"),
            Self::MissingSvgElement => write!(f, "no <svg> root element found"),
            Self::InvalidViewBox(e) => write!(f, "invalid viewBox: {e}"),
            Self::InvalidPathData { detail, position } => {
                write!(f, "invalid path data at position {position}: {detail}")
            }
            Self::InvalidTransform(e) => write!(f, "invalid transform: {e}"),
        }
    }
}

impl std::error::Error for SvgParseError {}

/// A parsed SVG icon: merged path geometry + viewBox, ready to be
/// scaled and rendered. All original fill/stroke colors are stripped.
#[derive(Debug, Clone)]
pub struct SvgIcon {
    /// Merged path in viewBox coordinates.
    path: Path,
    /// The SVG viewBox (defines the coordinate space).
    view_box: Rect,
}

impl SvgIcon {
    /// Parse an SVG string into an `SvgIcon`.
    pub fn parse(svg_str: &str) -> Result<Self, SvgParseError> {
        let doc = roxmltree::Document::parse(svg_str)
            .map_err(|e| SvgParseError::XmlError(e.to_string()))?;

        let svg_el = doc
            .root_element()
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "svg")
            .or_else(|| {
                let root = doc.root_element();
                if root.tag_name().name() == "svg" {
                    Some(root)
                } else {
                    None
                }
            })
            .ok_or(SvgParseError::MissingSvgElement)?;

        let view_box = parse_view_box(&svg_el)?;

        let mut merged = Path::new();
        walk_element(&svg_el, &Transform2D::IDENTITY, &mut merged)?;

        Ok(SvgIcon {
            path: merged,
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

    /// Produce a [`Path`] scaled to fit within `rect`, preserving aspect
    /// ratio and centering.
    pub fn to_path_in_rect(&self, rect: Rect) -> Path {
        if self.path.is_empty() || self.view_box.width <= 0.0 || self.view_box.height <= 0.0 {
            return Path::new();
        }
        let scale_x = rect.width / self.view_box.width;
        let scale_y = rect.height / self.view_box.height;
        let scale = scale_x.min(scale_y);
        let scaled_w = self.view_box.width * scale;
        let scaled_h = self.view_box.height * scale;
        let offset_x = rect.x + (rect.width - scaled_w) / 2.0;
        let offset_y = rect.y + (rect.height - scaled_h) / 2.0;

        // Scale viewBox coordinates first, then translate to target position.
        let transform = Transform2D::scale(scale, scale).then(&Transform2D::translate(
            offset_x - self.view_box.x * scale,
            offset_y - self.view_box.y * scale,
        ));

        self.path.transformed(&transform)
    }

    /// Access the raw path in viewBox coordinates.
    pub fn raw_path(&self) -> &Path {
        &self.path
    }

    /// Access the viewBox.
    pub fn view_box(&self) -> Rect {
        self.view_box
    }
}

// --- Internal helpers ---

fn parse_view_box(svg_el: &roxmltree::Node) -> Result<Rect, SvgParseError> {
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

fn walk_element(
    node: &roxmltree::Node,
    parent_transform: &Transform2D,
    merged: &mut Path,
) -> Result<(), SvgParseError> {
    let transform = if let Some(t_attr) = node.attribute("transform") {
        let local = parse_transform(t_attr)?;
        parent_transform.then(&local)
    } else {
        *parent_transform
    };

    let tag = node.tag_name().name();
    match tag {
        "path" => {
            if let Some(d) = node.attribute("d") {
                let cmds = path_parser::parse_svg_path_data(d)?;
                let mut sub = Path::new();
                sub.commands = cmds;
                if transform != Transform2D::IDENTITY {
                    let transformed = sub.transformed(&transform);
                    merged.append(&transformed);
                } else {
                    merged.append(&sub);
                }
            }
        }
        "rect" => {
            if let Some(path) = parse_rect_element(node) {
                append_transformed(merged, &path, &transform);
            }
        }
        "circle" => {
            if let Some(path) = parse_circle_element(node) {
                append_transformed(merged, &path, &transform);
            }
        }
        "ellipse" => {
            if let Some(path) = parse_ellipse_element(node) {
                append_transformed(merged, &path, &transform);
            }
        }
        "line" => {
            if let Some(path) = parse_line_element(node) {
                append_transformed(merged, &path, &transform);
            }
        }
        "polygon" => {
            if let Some(path) = parse_polygon_element(node) {
                append_transformed(merged, &path, &transform);
            }
        }
        "polyline" => {
            if let Some(path) = parse_polyline_element(node) {
                append_transformed(merged, &path, &transform);
            }
        }
        _ => {}
    }

    // Recurse into children (for <g>, <svg>, <defs>, etc.)
    for child in node.children().filter(|c| c.is_element()) {
        walk_element(&child, &transform, merged)?;
    }

    Ok(())
}

fn append_transformed(merged: &mut Path, path: &Path, transform: &Transform2D) {
    if *transform != Transform2D::IDENTITY {
        merged.append(&path.transformed(transform));
    } else {
        merged.append(path);
    }
}

// --- Shape element parsers ---

fn parse_rect_element(node: &roxmltree::Node) -> Option<Path> {
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
            fern_tokens::CornerRadius::uniform(r),
        ))
    } else {
        Some(Path::rect(Rect::new(x, y, w, h)))
    }
}

fn parse_circle_element(node: &roxmltree::Node) -> Option<Path> {
    let cx = attr_f32(node, "cx").unwrap_or(0.0);
    let cy = attr_f32(node, "cy").unwrap_or(0.0);
    let r = attr_f32(node, "r")?;
    Some(Path::circle(Point::new(cx, cy), r))
}

fn parse_ellipse_element(node: &roxmltree::Node) -> Option<Path> {
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

fn parse_line_element(node: &roxmltree::Node) -> Option<Path> {
    let x1 = attr_f32(node, "x1").unwrap_or(0.0);
    let y1 = attr_f32(node, "y1").unwrap_or(0.0);
    let x2 = attr_f32(node, "x2").unwrap_or(0.0);
    let y2 = attr_f32(node, "y2").unwrap_or(0.0);
    Some(Path::line(Point::new(x1, y1), Point::new(x2, y2)))
}

fn parse_polygon_element(node: &roxmltree::Node) -> Option<Path> {
    let points = parse_points_attr(node)?;
    Some(Path::polygon(&points))
}

fn parse_polyline_element(node: &roxmltree::Node) -> Option<Path> {
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

fn parse_points_attr(node: &roxmltree::Node) -> Option<Vec<Point>> {
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

fn attr_f32(node: &roxmltree::Node, name: &str) -> Option<f32> {
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
}
