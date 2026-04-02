use fern_tokens::Color;

use crate::geometry::Point;

/// A paint type for filling shapes.
#[derive(Debug, Clone, PartialEq)]
pub enum Paint {
    Solid(Color),
    LinearGradient {
        start: Point,
        end: Point,
        stops: Vec<GradientStop>,
    },
    RadialGradient {
        center: Point,
        radius: f32,
        stops: Vec<GradientStop>,
    },
    ConicGradient {
        center: Point,
        start_angle: f32,
        stops: Vec<GradientStop>,
    },
    Image(ImageHandle),
}

/// Handle to an image resource.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageHandle {
    pub name: String,
}

/// A single stop in a gradient.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientStop {
    pub offset: f32,
    pub color: Color,
}

/// Line cap style for strokes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineCap {
    #[default]
    Butt,
    Round,
    Square,
}

/// Stroke style configuration, supporting dashed/dotted strokes.
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeStyle {
    pub width: f32,
    /// Alternating dash/gap lengths. `None` means solid stroke.
    pub dash_pattern: Option<Vec<f32>>,
    /// Phase offset for the dash pattern.
    pub dash_offset: f32,
    pub line_cap: LineCap,
}

impl StrokeStyle {
    pub fn solid(width: f32) -> Self {
        Self {
            width,
            dash_pattern: None,
            dash_offset: 0.0,
            line_cap: LineCap::Butt,
        }
    }

    pub fn dashed(width: f32, dash: f32, gap: f32) -> Self {
        Self {
            width,
            dash_pattern: Some(vec![dash, gap]),
            dash_offset: 0.0,
            line_cap: LineCap::Butt,
        }
    }

    pub fn dotted(width: f32, spacing: f32) -> Self {
        Self {
            width,
            dash_pattern: Some(vec![width, spacing]),
            dash_offset: 0.0,
            line_cap: LineCap::Round,
        }
    }
}

impl From<f32> for StrokeStyle {
    fn from(width: f32) -> Self {
        Self::solid(width)
    }
}

impl From<Color> for Paint {
    fn from(color: Color) -> Self {
        Paint::Solid(color)
    }
}
