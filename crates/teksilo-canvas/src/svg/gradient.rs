// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! SVG gradient paint servers — `<linearGradient>` / `<radialGradient>`.
//!
//! A gradient is declared once (usually in `<defs>`) and referenced by any
//! number of shapes as `fill="url(#id)"`. This module parses the declarations
//! into [`GradientDef`]s; [`resolve`](GradientDef::resolve) then bakes one into
//! the *shape's* coordinate space, because that is what SVG's default
//! `gradientUnits="objectBoundingBox"` means: the gradient's coordinates are
//! fractions of the bounding box of whichever shape is painted with it, so the
//! same declaration lands differently on every shape that references it.
//!
//! **`href` inheritance is part of the format, not a nicety.** The canonical way
//! to author "the same ramp, three different directions" is one gradient holding
//! the `<stop>`s and three that inherit them via `xlink:href`. A parser that
//! ignores `href` doesn't render those gradients slightly wrong — it renders
//! them as nothing, because the inheriting gradients have no stops of their own.
//!
//! **Known limits**, all inherited from what the renderer's gradient pipeline
//! can express, and all documented on [`SvgIcon`](super::SvgIcon):
//! `spreadMethod` is always `pad` (`reflect` / `repeat` are ignored); a radial
//! gradient's focal point (`fx` / `fy`) is ignored, so it stays concentric; and
//! an `objectBoundingBox` radial on a non-square box is drawn as a circle of the
//! equivalent area rather than as the ellipse SVG specifies.

use std::collections::HashMap;

use teksilo_tokens::Color;

use crate::geometry::{Point, Rect, Transform2D};
use crate::xml::XmlElement;

use super::color::{parse_alpha, parse_color};
use super::{parse_inline_style, parse_transform};

/// One stop of an SVG gradient ramp.
///
/// The color is an `Option` for the same reason [`SvgPaint::Current`] exists: a
/// stop may be authored `stop-color="currentColor"`, and the tint that resolves
/// to isn't known until the widget paints. Baking it to black at parse time —
/// the obvious shortcut — silently turns a themed ramp into a black one.
///
/// [`SvgPaint::Current`]: super::SvgPaint::Current
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvgStop {
    /// Position along the ramp, `0..=1`.
    pub offset: f32,
    /// The stop's color, or `None` for `currentColor`.
    pub color: Option<Color>,
    /// `stop-opacity`, kept apart from the color's own alpha so it can also
    /// attenuate a `currentColor` stop.
    pub opacity: f32,
}

/// How a gradient's coordinates are interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GradientUnits {
    /// Fractions of the painted shape's bounding box (SVG's default).
    ObjectBoundingBox,
    /// The user space in effect where the gradient is *referenced*.
    UserSpaceOnUse,
}

/// The geometry of a parsed gradient, in its own (unresolved) coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
enum GradientGeometry {
    Linear { x1: f32, y1: f32, x2: f32, y2: f32 },
    Radial { cx: f32, cy: f32, r: f32 },
}

/// A gradient paint server, parsed but not yet bound to a shape.
#[derive(Debug, Clone)]
pub(crate) struct GradientDef {
    geometry: GradientGeometry,
    stops: Vec<SvgStop>,
    units: GradientUnits,
    /// `gradientTransform` — applied to the gradient's coordinates *before*
    /// the object-bounding-box mapping and the element's own transform.
    transform: Transform2D,
}

/// A gradient resolved into a concrete coordinate space: geometry in viewBox
/// units, ready to be fitted to a display rect alongside the geometry it paints.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedGradient {
    Linear {
        start: Point,
        end: Point,
        stops: Vec<SvgStop>,
    },
    Radial {
        center: Point,
        radius: f32,
        stops: Vec<SvgStop>,
    },
}

/// Depth cap for `href` chains — guards against a reference cycle
/// (`a href=#b`, `b href=#a`), which is malformed but must not hang the parser.
const MAX_HREF_DEPTH: usize = 16;

impl GradientDef {
    /// Bake this gradient into the coordinate space of the shape it paints.
    ///
    /// * `bbox` — the shape's bounding box in its *local* space (pre-transform),
    ///   which is what `objectBoundingBox` fractions multiply against. Per SVG,
    ///   this is the fill geometry's box: a stroke's width does not widen it.
    /// * `to_view_box` — the element's cumulative transform, mapping its local
    ///   space to the icon's viewBox.
    ///
    /// `None` when the gradient can't paint: no stops at all, or a degenerate
    /// `objectBoundingBox` (a zero-width/height shape, whose unit square has
    /// nowhere to map to).
    pub(crate) fn resolve(
        &self,
        bbox: Rect,
        to_view_box: &Transform2D,
    ) -> Option<ResolvedGradient> {
        if self.stops.is_empty() {
            return None;
        }
        // gradient coords → local space → viewBox space.
        let to_local = match self.units {
            GradientUnits::UserSpaceOnUse => self.transform,
            GradientUnits::ObjectBoundingBox => {
                if bbox.width <= 0.0 || bbox.height <= 0.0 {
                    return None;
                }
                // The unit square maps onto the bbox; gradientTransform acts
                // inside that unit space, so it composes first.
                let unit_to_bbox = Transform2D::scale(bbox.width, bbox.height)
                    .then(&Transform2D::translate(bbox.x, bbox.y));
                self.transform.then(&unit_to_bbox)
            }
        };
        let m = to_local.then(to_view_box);

        // A single stop is a solid color, but the ramp the shader samples needs
        // two ends; duplicating it is the cheapest correct representation.
        let stops = if self.stops.len() == 1 {
            let only = self.stops[0];
            vec![
                SvgStop {
                    offset: 0.0,
                    ..only
                },
                SvgStop {
                    offset: 1.0,
                    ..only
                },
            ]
        } else {
            self.stops.clone()
        };

        Some(match self.geometry {
            GradientGeometry::Linear { x1, y1, x2, y2 } => ResolvedGradient::Linear {
                start: m.apply_point(Point::new(x1, y1)),
                end: m.apply_point(Point::new(x2, y2)),
                stops,
            },
            GradientGeometry::Radial { cx, cy, r } => ResolvedGradient::Radial {
                center: m.apply_point(Point::new(cx, cy)),
                // One radius, so an anisotropic mapping (a non-square bbox, or a
                // scaling gradientTransform) is folded into its geometric mean —
                // the circle of the same area as SVG's ellipse.
                radius: r * m.geometric_scale(),
                stops,
            },
        })
    }
}

/// Parse every `<linearGradient>` / `<radialGradient>` in the document, keyed by
/// `id` and with `href` chains already flattened.
///
/// `view_box` resolves the percentages a `userSpaceOnUse` gradient may use
/// (`x1="50%"`), which are relative to the viewport.
pub(crate) fn collect_gradients<'a>(
    id_map: &HashMap<&'a str, &'a XmlElement>,
    view_box: Rect,
) -> HashMap<String, GradientDef> {
    let mut out = HashMap::new();
    for (id, el) in id_map {
        let tag = el.tag_name();
        if tag != "linearGradient" && tag != "radialGradient" {
            continue;
        }
        if let Some(def) = parse_gradient(el, id_map, view_box, 0) {
            out.insert((*id).to_string(), def);
        }
    }
    out
}

/// Parse one gradient element, folding in whatever it inherits via `href`.
fn parse_gradient<'a>(
    el: &'a XmlElement,
    id_map: &HashMap<&'a str, &'a XmlElement>,
    view_box: Rect,
    depth: usize,
) -> Option<GradientDef> {
    // The inherited base (stops + any attribute this element doesn't set).
    let inherited = if depth < MAX_HREF_DEPTH {
        el.attribute("href")
            .or_else(|| el.attribute("xlink:href"))
            .and_then(|h| h.strip_prefix('#'))
            .and_then(|id| id_map.get(id))
            .and_then(|target| parse_gradient(target, id_map, view_box, depth + 1))
    } else {
        None
    };

    let is_radial = el.tag_name() == "radialGradient";
    let units = match el.attribute("gradientUnits") {
        Some(u) if u.trim() == "userSpaceOnUse" => GradientUnits::UserSpaceOnUse,
        Some(_) => GradientUnits::ObjectBoundingBox,
        None => inherited
            .as_ref()
            .map(|g| g.units)
            .unwrap_or(GradientUnits::ObjectBoundingBox),
    };

    // A percentage means "of the viewport" in user space, but "of the unit
    // square" (i.e. just /100) in bounding-box space.
    let axis = |v: &str, span: f32| -> Option<f32> {
        let t = v.trim();
        match t.strip_suffix('%') {
            Some(p) => {
                let frac = p.trim().parse::<f32>().ok()? / 100.0;
                Some(match units {
                    GradientUnits::ObjectBoundingBox => frac,
                    GradientUnits::UserSpaceOnUse => frac * span,
                })
            }
            None => t.parse::<f32>().ok(),
        }
    };
    let get =
        |name: &str, span: f32| -> Option<f32> { el.attribute(name).and_then(|v| axis(v, span)) };

    // The diagonal is what SVG normalizes a radial `r="50%"` against.
    let diag = ((view_box.width.powi(2) + view_box.height.powi(2)) / 2.0).sqrt();

    let inherited_geo = inherited.as_ref().map(|g| g.geometry);
    let geometry = if is_radial {
        // Defaults: a circle filling the object's box (cx=cy=r=50%).
        let (icx, icy, ir) = match inherited_geo {
            Some(GradientGeometry::Radial { cx, cy, r }) => (Some(cx), Some(cy), Some(r)),
            _ => (None, None, None),
        };
        let default = |frac: f32, span: f32| match units {
            GradientUnits::ObjectBoundingBox => frac,
            GradientUnits::UserSpaceOnUse => frac * span,
        };
        GradientGeometry::Radial {
            cx: get("cx", view_box.width)
                .or(icx)
                .unwrap_or_else(|| default(0.5, view_box.width)),
            cy: get("cy", view_box.height)
                .or(icy)
                .unwrap_or_else(|| default(0.5, view_box.height)),
            r: get("r", diag).or(ir).unwrap_or_else(|| default(0.5, diag)),
        }
    } else {
        // Defaults: left-to-right across the object's box.
        let (ix1, iy1, ix2, iy2) = match inherited_geo {
            Some(GradientGeometry::Linear { x1, y1, x2, y2 }) => {
                (Some(x1), Some(y1), Some(x2), Some(y2))
            }
            _ => (None, None, None, None),
        };
        let default = |frac: f32, span: f32| match units {
            GradientUnits::ObjectBoundingBox => frac,
            GradientUnits::UserSpaceOnUse => frac * span,
        };
        GradientGeometry::Linear {
            x1: get("x1", view_box.width)
                .or(ix1)
                .unwrap_or_else(|| default(0.0, view_box.width)),
            y1: get("y1", view_box.height)
                .or(iy1)
                .unwrap_or_else(|| default(0.0, view_box.height)),
            x2: get("x2", view_box.width)
                .or(ix2)
                .unwrap_or_else(|| default(1.0, view_box.width)),
            y2: get("y2", view_box.height)
                .or(iy2)
                .unwrap_or_else(|| default(0.0, view_box.height)),
        }
    };

    let transform = el
        .attribute("gradientTransform")
        .and_then(|t| parse_transform(t).ok())
        .or_else(|| inherited.as_ref().map(|g| g.transform))
        .unwrap_or(Transform2D::IDENTITY);

    let mut stops = parse_stops(el);
    if stops.is_empty() {
        // No stops of its own: this gradient exists to vary the *geometry* of an
        // inherited ramp — the `href` idiom this parser must not drop.
        stops = inherited.map(|g| g.stops).unwrap_or_default();
    }

    Some(GradientDef {
        geometry,
        stops,
        units,
        transform,
    })
}

/// Parse a gradient's `<stop>` children.
///
/// Offsets are clamped to `0..=1` and forced non-decreasing: SVG requires each
/// stop to sit at or after its predecessor, and a ramp that walks backwards
/// renders as garbage rather than as an error. `stop-color: currentColor` stays
/// unresolved (see [`SvgStop`]).
fn parse_stops(gradient: &XmlElement) -> Vec<SvgStop> {
    let mut out: Vec<SvgStop> = Vec::new();
    for stop in gradient.children().filter(|c| c.tag_name() == "stop") {
        // SVG's initial stop-color is black; `currentColor` defers to the tint.
        let read_color = |v: &str| -> Option<Option<Color>> {
            if v.trim().eq_ignore_ascii_case("currentcolor") {
                Some(None)
            } else {
                parse_color(v).map(Some)
            }
        };

        // Presentation attributes, then the inline `style` (which wins).
        let mut color = stop
            .attribute("stop-color")
            .and_then(read_color)
            .unwrap_or(Some(Color::BLACK));
        let mut opacity = stop
            .attribute("stop-opacity")
            .and_then(parse_alpha)
            .unwrap_or(1.0);
        if let Some(style) = stop.attribute("style") {
            for (key, value) in parse_inline_style(style) {
                match key {
                    "stop-color" => {
                        if let Some(c) = read_color(value) {
                            color = c;
                        }
                    }
                    "stop-opacity" => {
                        if let Some(o) = parse_alpha(value) {
                            opacity = o;
                        }
                    }
                    _ => {}
                }
            }
        }

        let offset = stop
            .attribute("offset")
            .and_then(|o| {
                let t = o.trim();
                match t.strip_suffix('%') {
                    Some(p) => p.trim().parse::<f32>().ok().map(|v| v / 100.0),
                    None => t.parse::<f32>().ok(),
                }
            })
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        let offset = match out.last() {
            Some(prev) => offset.max(prev.offset),
            None => offset,
        };

        out.push(SvgStop {
            offset,
            color,
            opacity,
        });
    }
    out
}
