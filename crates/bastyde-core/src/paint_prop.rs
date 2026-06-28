// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `PaintProp` — a fill input that may be a flat color *or* a gradient.
//!
//! [`ColorProp`] covers the flat case (static color / theme role /
//! `Signal<Color>`). `PaintProp` is the superset that a fill-bearing
//! widget (`RectWidget`) accepts so a theme recipe can describe a
//! gradient (`FillRecipe::LinearGradient` / `RadialGradient`). Anything
//! that is `Into<ColorProp>` is also `Into<PaintProp>` (as a solid), so
//! callers that pass a `Color` / role / signal keep working unchanged.
//!
//! Gradient stop colors are themselves [`ColorProp`]s, so they resolve
//! against the live theme at paint time. Gradient geometry is computed
//! from the widget's **size** at paint (the canvas [`Paint`] gradient
//! coordinates are rect-local), so a `PaintProp` never needs the widget's
//! absolute position.

use bastyde_canvas::{GradientStop, Paint, Point, Size};

use crate::binding::{BindingLevel, BindingRegistry};
use crate::color_prop::ColorProp;
use crate::styles::{FillRecipe, Theme};
use crate::widget_id::WidgetId;

/// One stop of a gradient `PaintProp`. The color is a [`ColorProp`] so it
/// tracks theme/role/signal changes.
#[derive(Clone, Debug)]
pub struct GradientStopProp {
    /// Position along the gradient axis, `0.0..=1.0`.
    pub offset: f32,
    pub color: ColorProp,
}

/// A fill that resolves to a canvas [`Paint`] — flat or gradient.
#[derive(Clone, Debug)]
pub enum PaintProp {
    /// Flat fill — delegates to [`ColorProp`] (the common case; fully
    /// reactive via a bound color or role).
    Solid(ColorProp),
    /// Linear gradient. `angle_deg`: `0°` = top→bottom, `90°` =
    /// leading→trailing.
    Linear {
        stops: Vec<GradientStopProp>,
        angle_deg: f32,
    },
    /// Radial gradient. `center` is normalized (`0.0..=1.0`) within the
    /// rect; `radius` is normalized to the rect's longer side.
    Radial {
        stops: Vec<GradientStopProp>,
        center: (f32, f32),
        radius: f32,
    },
}

impl PaintProp {
    /// A solid fill from anything `Into<ColorProp>`.
    pub fn solid(color: impl Into<ColorProp>) -> Self {
        PaintProp::Solid(color.into())
    }

    /// Build a `PaintProp` from a [`FillRecipe`]. Flat variants
    /// (`Solid`/`StateLayer`/`None`) become a `Solid` `ColorProp`;
    /// gradients map their `RecipeColor` stops to `ColorProp` stops.
    /// `StateLayer` can't be a single role, so it resolves to a frozen
    /// composited color (still re-resolved on theme swap only if the
    /// caller rebuilds — recipe styles instead fold state layers into a
    /// reactive `Solid` via their own state signal).
    pub fn from_fill(fill: &FillRecipe, colors: &bastyde_tokens::ColorTokens) -> Self {
        match fill {
            FillRecipe::Solid(c) => PaintProp::Solid((*c).into()),
            FillRecipe::None => {
                PaintProp::Solid(ColorProp::Static(bastyde_tokens::Color::TRANSPARENT))
            }
            FillRecipe::StateLayer { .. } => {
                // Pre-composite against the supplied tokens (frozen).
                let flat = fill
                    .resolve_flat(colors)
                    .unwrap_or(bastyde_tokens::Color::TRANSPARENT);
                PaintProp::Solid(ColorProp::Static(flat))
            }
            FillRecipe::LinearGradient { stops, angle_deg } => PaintProp::Linear {
                stops: stops.iter().map(stop_to_prop).collect(),
                angle_deg: *angle_deg,
            },
            FillRecipe::RadialGradient {
                stops,
                center,
                radius,
            } => PaintProp::Radial {
                stops: stops.iter().map(stop_to_prop).collect(),
                center: *center,
                radius: *radius,
            },
        }
    }

    /// Resolve to a canvas [`Paint`]. `size` is the filled rect's size;
    /// gradient endpoints are rect-local (`(0,0)` = top-left).
    pub fn resolve(&self, theme: &Theme, enabled: bool, size: Size) -> Paint {
        match self {
            PaintProp::Solid(c) => Paint::Solid(c.resolve(theme, enabled)),
            PaintProp::Linear { stops, angle_deg } => {
                let (start, end) = angle_to_endpoints(*angle_deg, size);
                Paint::LinearGradient {
                    start,
                    end,
                    stops: resolve_stops(stops, theme, enabled),
                }
            }
            PaintProp::Radial {
                stops,
                center,
                radius,
            } => Paint::RadialGradient {
                center: Point::new(center.0 * size.width, center.1 * size.height),
                radius: radius * size.width.max(size.height),
                stops: resolve_stops(stops, theme, enabled),
            },
        }
    }

    /// Register dirty-tracking for any signal-bearing color (the solid
    /// color or each gradient stop).
    pub fn register_if_bound(
        &self,
        widget_id: WidgetId,
        registry: &BindingRegistry,
        level: BindingLevel,
    ) {
        match self {
            PaintProp::Solid(c) => c.register_if_bound(widget_id, registry, level),
            PaintProp::Linear { stops, .. } | PaintProp::Radial { stops, .. } => {
                for s in stops {
                    s.color.register_if_bound(widget_id, registry, level);
                }
            }
        }
    }
}

fn stop_to_prop(s: &crate::styles::GradientStop) -> GradientStopProp {
    GradientStopProp {
        offset: s.offset,
        color: s.color.into(),
    }
}

fn resolve_stops(stops: &[GradientStopProp], theme: &Theme, enabled: bool) -> Vec<GradientStop> {
    stops
        .iter()
        .map(|s| GradientStop {
            offset: s.offset,
            color: s.color.resolve(theme, enabled),
        })
        .collect()
}

/// Map a gradient angle (degrees; `0°` = top→bottom, `90°` =
/// leading→trailing) to rect-local start/end points through the rect
/// centre. Axis-aligned angles land exactly on the conventional edges.
pub fn angle_to_endpoints(angle_deg: f32, size: Size) -> (Point, Point) {
    let rad = angle_deg.to_radians();
    // dir: 0° → (0, 1) down, 90° → (1, 0) right.
    let dx = rad.sin();
    let dy = rad.cos();
    let cx = size.width * 0.5;
    let cy = size.height * 0.5;
    let ex = dx * size.width * 0.5;
    let ey = dy * size.height * 0.5;
    (Point::new(cx - ex, cy - ey), Point::new(cx + ex, cy + ey))
}

// Anything `Into<ColorProp>` is a solid `PaintProp` — keeps existing
// `.background(color/role/signal)` callers working after the field type
// changes from `ColorProp` to `PaintProp`.
impl<T: Into<ColorProp>> From<T> for PaintProp {
    fn from(t: T) -> Self {
        PaintProp::Solid(t.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::intui;
    use crate::styles::{GradientStop as RecipeStop, RecipeColor};
    use bastyde_tokens::{Color, SurfaceRole};

    #[test]
    fn solid_from_color_resolves() {
        let theme = intui::light();
        let p = PaintProp::solid(Color::RED);
        match p.resolve(&theme, true, Size::new(10.0, 10.0)) {
            Paint::Solid(c) => assert_eq!(c, Color::RED),
            _ => panic!("expected solid"),
        }
    }

    #[test]
    fn vertical_angle_endpoints() {
        // 0° top→bottom over a 100×40 rect.
        let (s, e) = angle_to_endpoints(0.0, Size::new(100.0, 40.0));
        assert_eq!((s.x, s.y), (50.0, 0.0));
        assert_eq!((e.x, e.y), (50.0, 40.0));
        // 90° leading→trailing.
        let (s, e) = angle_to_endpoints(90.0, Size::new(100.0, 40.0));
        assert!((s.x - 0.0).abs() < 1e-4 && (s.y - 20.0).abs() < 1e-4);
        assert!((e.x - 100.0).abs() < 1e-4 && (e.y - 20.0).abs() < 1e-4);
    }

    #[test]
    fn linear_gradient_from_fill_resolves_role_stops() {
        let theme = intui::light();
        let fill = FillRecipe::LinearGradient {
            stops: vec![
                RecipeStop {
                    offset: 0.0,
                    color: RecipeColor::Surface(SurfaceRole::Accent),
                },
                RecipeStop {
                    offset: 1.0,
                    color: RecipeColor::Static(Color::WHITE),
                },
            ],
            angle_deg: 0.0,
        };
        let p = PaintProp::from_fill(&fill, &theme.colors);
        match p.resolve(&theme, true, Size::new(20.0, 20.0)) {
            Paint::LinearGradient { stops, .. } => {
                assert_eq!(stops.len(), 2);
                assert_eq!(stops[0].color, theme.colors.accent);
                assert_eq!(stops[1].color, Color::WHITE);
            }
            _ => panic!("expected linear gradient"),
        }
    }
}
